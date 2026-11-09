//! The model behind the actions: Apple's on-device model ([`on_device`]), a
//! ChatGPT plan through the sign-in Codex uses ([`chatgpt`]), or any
//! OpenAI-compatible API ([`compat`]). All stream the answer.

pub mod chatgpt;
mod compat;
pub mod on_device;
mod sse;

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;

use crate::settings::{ChatGptAuth, Provider, Settings};

/// The longest an answer may take in all: the HTTP client only limits each
/// read, so a server that keeps the stream open without finishing it could
/// hold a request, and the check or dictation waiting on it, forever.
const MAX_ANSWER: Duration = Duration::from_secs(300);

/// One request: the system prompt and the text it applies to.
pub struct Ask {
    pub system: String,
    pub text: String,
    /// The model to ask; `None` on device, which has only one.
    pub model: Option<String>,
    pub effort: Effort,
}

/// How much the model reasons before it answers, as the APIs name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Low,
}

impl Ask {
    /// Return the model to ask, which every backend but the on-device one needs.
    fn model(&self) -> Result<&str> {
        self.model.as_deref().context("no model is chosen")
    }
}

/// What the settings lack before the model can be called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    OnDevice(on_device::Availability),
    SignIn,
    ApiUrl,
    Model,
}

/// Where requests go, with what they need.
#[derive(Clone)]
pub enum Backend {
    OnDevice,
    ChatGpt(ChatGptAuth),
    Api { base: String, key: String },
}

impl Backend {
    /// Where `settings` send requests, or what they lack to send any.
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

    /// Where `settings` send a request and the model they ask, or what they
    /// lack to send one.
    pub fn with_model(settings: &Settings) -> Result<(Self, Option<String>), Missing> {
        let backend = Self::of(settings)?;
        match settings.model().map(str::trim) {
            Some("") => Err(Missing::Model),
            model => Ok((backend, model.map(str::to_owned))),
        }
    }

    /// Stream the answer to `ask`, calling `on_delta` with each piece.
    ///
    /// # Errors
    ///
    /// Fails if the backend does, or if the answer takes longer than [`MAX_ANSWER`].
    pub async fn stream(
        &self,
        http: &reqwest::Client,
        ask: &Ask,
        on_delta: impl FnMut(&str),
    ) -> Result<()> {
        let answer = async {
            match self {
                Backend::OnDevice => on_device::stream(ask, on_delta).await,
                Backend::ChatGpt(auth) => chatgpt::stream(http, auth, ask, on_delta).await,
                Backend::Api { base, key } => compat::stream(http, base, key, ask, on_delta).await,
            }
        };
        tokio::time::timeout(MAX_ANSWER, answer)
            .await
            .map_err(|_| anyhow!("the model took too long to answer"))?
    }

    /// The models this backend offers, sorted; none to pick from on device.
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
