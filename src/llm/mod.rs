pub mod chatgpt;
mod compat;
pub mod on_device;
mod sse;

use std::fmt;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::Value;

use crate::settings::{ChatGptAuth, Provider, Settings};

/// A per-read HTTP timeout can't stop a stream that stays open without completing.
const MAX_ANSWER: Duration = Duration::from_secs(300);

pub struct Ask {
    pub system: String,
    pub text: String,
    /// The model to ask; `None` on device, which has only one.
    pub model: Option<String>,
    pub effort: Effort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Low,
}

impl Ask {
    fn model(&self) -> Result<&str> {
        self.model.as_deref().context("no model is chosen")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    OnDevice(on_device::Availability),
    SignIn,
    ApiUrl,
    Model,
}

#[derive(Clone)]
pub enum Backend {
    OnDevice,
    ChatGpt(ChatGptAuth),
    Api { base: String, key: String },
}

impl Backend {
    pub fn of(settings: &Settings) -> Result<Self, Missing> {
        match settings.provider {
            Provider::OnDevice => match on_device::availability() {
                on_device::Availability::Ready => Ok(Backend::OnDevice),
                other => Err(Missing::OnDevice(other)),
            },
            Provider::ChatGpt => settings
                .credentials
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
                    key: settings.credentials.api_key.trim().to_owned(),
                })
            }
        }
    }

    pub fn with_model(settings: &Settings) -> Result<(Self, Option<String>), Missing> {
        let backend = Self::of(settings)?;
        match settings.model().map(str::trim) {
            Some("") => Err(Missing::Model),
            model => Ok((backend, model.map(str::to_owned))),
        }
    }

    /// Retry without an effort only when the model rejects the requested reasoning setting.
    pub async fn stream(
        &self,
        http: &reqwest::Client,
        ask: &Ask,
        mut on_delta: impl FnMut(&str),
    ) -> Result<()> {
        let answer = async {
            // The rejection comes before any piece, so asking again repeats none.
            match self.send(http, ask, Some(ask.effort), &mut on_delta).await {
                Err(e) if rejects_effort(&e) => self.send(http, ask, None, on_delta).await,
                other => other,
            }
        };
        tokio::time::timeout(MAX_ANSWER, answer)
            .await
            .map_err(|_| anyhow!("the model took too long to answer"))?
    }

    async fn send(
        &self,
        http: &reqwest::Client,
        ask: &Ask,
        effort: Option<Effort>,
        on_delta: impl FnMut(&str),
    ) -> Result<()> {
        match self {
            Backend::OnDevice => on_device::stream(ask, on_delta).await,
            Backend::ChatGpt(auth) => chatgpt::stream(http, auth, ask, effort, on_delta).await,
            Backend::Api { base, key } => {
                compat::stream(http, base, key, ask, effort, on_delta).await
            }
        }
    }

    pub async fn models(&self, http: &reqwest::Client) -> Result<Vec<String>> {
        let mut models = match self {
            Backend::OnDevice => Vec::new(),
            Backend::ChatGpt(auth) => chatgpt::models(http, auth).await?,
            Backend::Api { base, key } => compat::models(http, base, key).await?,
        };
        models.sort();
        models.dedup();
        Ok(models)
    }
}

pub fn http() -> Result<reqwest::Client> {
    // reqwest takes the process-wide provider; an Err means it's already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(format!("fettle/{}", crate::VERSION))
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(120))
        .build()
        .context("setting up HTTP")
}

#[derive(Debug)]
struct Refused {
    status: reqwest::StatusCode,
    message: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.status)
    }
}

impl std::error::Error for Refused {}

fn rejects_effort(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Refused>().is_some_and(|r| {
        r.status == reqwest::StatusCode::BAD_REQUEST
            && r.message.to_lowercase().contains("reasoning")
    })
}

async fn check(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(Refused {
        status,
        message: error_message(&body),
    }
    .into())
}

fn error_message(body: &str) -> String {
    let json: Option<Value> = serde_json::from_str(body).ok();
    let found = json.as_ref().and_then(|v| {
        [
            "/error/message",
            "/error",
            "/detail/message",
            "/detail",
            "/message",
        ]
        .into_iter()
        .find_map(|path| v.pointer(path)?.as_str())
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
