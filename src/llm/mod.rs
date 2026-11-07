//! The model behind Fix and Translate: a ChatGPT plan through the sign-in
//! Codex uses ([`chatgpt`]), or any OpenAI-compatible API ([`compat`]). Both
//! stream the answer.

pub mod chatgpt;
mod compat;
mod sse;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::settings::{ChatGptAuth, Missing, Provider, Settings};

/// One request: the system prompt and the text it applies to.
pub struct Ask {
    pub system: String,
    pub text: String,
    pub model: String,
    /// The reasoning effort, or `None` for the model's default.
    pub effort: Option<&'static str>,
}

/// Where requests go, with what they need.
#[derive(Clone)]
pub enum Backend {
    ChatGpt(ChatGptAuth),
    Api { base: String, key: String },
}

impl Backend {
    /// Where `settings` send requests, or what they lack to send any.
    pub fn of(settings: &Settings) -> Result<Self, Missing> {
        match settings.provider {
            Provider::ChatGpt => settings
                .chatgpt
                .clone()
                .map(Backend::ChatGpt)
                .ok_or(Missing::SignIn),
            Provider::Api => {
                let base = settings.api_base.trim().trim_end_matches('/');
                if base.is_empty() {
                    return Err(Missing::ApiUrl);
                }
                Ok(Backend::Api {
                    base: base.to_owned(),
                    key: settings.api_key.trim().to_owned(),
                })
            }
        }
    }

    /// Stream the answer to `ask`, calling `on_delta` with each piece.
    pub async fn stream(
        &self,
        http: &reqwest::Client,
        ask: &Ask,
        on_delta: impl FnMut(&str),
    ) -> Result<()> {
        match self {
            Backend::ChatGpt(auth) => chatgpt::stream(http, auth, ask, on_delta).await,
            Backend::Api { base, key } => compat::stream(http, base, key, ask, on_delta).await,
        }
    }

    /// The models this backend offers, sorted.
    pub async fn models(&self, http: &reqwest::Client) -> Result<Vec<String>> {
        let mut models = match self {
            Backend::ChatGpt(auth) => chatgpt::models(http, auth).await?,
            Backend::Api { base, key } => compat::models(http, base, key).await?,
        };
        models.sort();
        models.dedup();
        Ok(models)
    }
}

/// The HTTP client for every call: generous on the whole answer, quick to
/// give up on connecting.
pub fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("fettle/{}", crate::VERSION))
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(120))
        .build()
        .context("setting up HTTP")
}

/// `response` if it succeeded, else an error with the API's own message.
async fn check(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    bail!("{} ({status})", error_message(&body))
}

/// The message in an API error body: OpenAI's `error.message`, a `detail`, or
/// the body itself.
fn error_message(body: &str) -> String {
    let json: Option<Value> = serde_json::from_str(body).ok();
    let found = json.as_ref().and_then(|v| {
        v.pointer("/error/message")
            .or_else(|| v.get("error"))
            .or_else(|| v.pointer("/detail/message"))
            .or_else(|| v.get("detail"))
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let text = found.unwrap_or_else(|| body.trim().to_owned());
    if text.is_empty() {
        "the server gave no reason".to_owned()
    } else {
        text.chars().take(300).collect()
    }
}

#[cfg(test)]
mod tests;
