//! Calls to the model from the window: each takes a fresh ChatGPT sign-in
//! first, refreshing and saving it through the window's state when it
//! expires soon.

use std::future::Future;
use std::sync::Arc;

use anyhow::{Context, Result};
use rune::AppHandle;
use tokio::runtime::Handle;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use super::state::State;
use crate::llm::{Ask, Backend, chatgpt};
use crate::settings::ChatGptAuth;

/// What the window's background work needs: the window, the runtime, and HTTP.
#[derive(Clone)]
pub struct Client {
    app: AppHandle<State>,
    rt: Handle,
    http: reqwest::Client,
    /// Held while the ChatGPT sign-in refreshes, so refreshes take turns.
    refreshing: Arc<Mutex<()>>,
}

impl Client {
    pub fn new(app: AppHandle<State>, rt: Handle, http: reqwest::Client) -> Self {
        Self {
            app,
            rt,
            http,
            refreshing: Arc::default(),
        }
    }

    pub fn app(&self) -> &AppHandle<State> {
        &self.app
    }

    pub fn rt(&self) -> &Handle {
        &self.rt
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Run `task` on the runtime.
    pub fn spawn<F>(&self, task: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.rt.spawn(task)
    }

    /// Stream the answer to `ask` from `backend`, calling `on_delta` with each piece.
    pub async fn stream(
        &self,
        backend: Backend,
        ask: &Ask,
        on_delta: impl FnMut(&str),
    ) -> Result<()> {
        let backend = self.fresh(backend).await?;
        backend.stream(&self.http, ask, on_delta).await
    }

    /// Return the whole answer to `ask` from `backend`.
    pub async fn answer(&self, backend: Backend, ask: &Ask) -> Result<String> {
        let mut answer = String::new();
        self.stream(backend, ask, |delta| answer.push_str(delta))
            .await?;
        Ok(answer)
    }

    /// Return the models `backend` offers, sorted.
    pub async fn models(&self, backend: Backend) -> Result<Vec<String>> {
        let backend = self.fresh(backend).await?;
        backend.models(&self.http).await
    }

    /// Return `backend` with a ChatGPT token that won't expire soon.
    async fn fresh(&self, backend: Backend) -> Result<Backend> {
        let Backend::ChatGpt(auth) = &backend else {
            return Ok(backend);
        };
        if !chatgpt::expiring(auth) {
            return Ok(backend);
        }
        // In a task of its own, so stopping the call that needed it can't
        // drop the new sign-in: the old refresh token is spent once OpenAI
        // answers.
        let client = self.clone();
        let auth = self
            .rt
            .spawn(async move { client.refresh().await })
            .await??;
        Ok(Backend::ChatGpt(auth))
    }

    /// Refresh the sign-in the settings hold, unless an earlier refresh
    /// already did, and save it.
    ///
    /// A refresh token works once, so refreshes take turns on `refreshing`,
    /// and each starts from the sign-in the settings hold by then.
    async fn refresh(&self) -> Result<ChatGptAuth> {
        let _turn = self.refreshing.lock().await;
        let (tx, rx) = oneshot::channel();
        self.app.update(move |s, _| {
            let _ = tx.send(s.settings.credentials.chatgpt.clone());
        });
        let auth = rx
            .await
            .ok()
            .flatten()
            .context("signed out of ChatGPT: sign in again in Settings")?;
        if !chatgpt::expiring(&auth) {
            return Ok(auth);
        }
        let fresh = chatgpt::refresh(&self.http, &auth).await?;
        let saved = fresh.clone();
        // Queued before the turn ends, so the next refresh reads this sign-in.
        self.app
            .update(move |s, _| s.saved_auth(&auth.refresh, saved));
        Ok(fresh)
    }
}
