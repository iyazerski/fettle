//! An OpenAI-compatible API: streamed chat completions and the model list.

use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use super::sse::{self, Flow};
use super::{Ask, Effort, check, error_message};

/// Stream the answer to `ask` with reasoning `effort`, or the model's default for `None`.
pub async fn stream(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    ask: &Ask,
    effort: Option<Effort>,
    mut on_delta: impl FnMut(&str),
) -> Result<()> {
    let mut body = json!({
        "model": ask.model()?,
        "stream": true,
        "messages": [
            { "role": "system", "content": ask.system },
            { "role": "user", "content": ask.text },
        ],
    });
    if let Some(effort) = effort {
        body["reasoning_effort"] = json!(effort);
    }
    let mut request = http.post(format!("{base}/chat/completions")).json(&body);
    if !key.is_empty() {
        request = request.bearer_auth(key);
    }
    let response = check(request.send().await?).await?;
    // Some servers end the stream without `[DONE]`, so a `stop` finish ends it too.
    sse::each(response, |data| {
        if data == "[DONE]" {
            return Ok(Flow::Done);
        }
        let event: Value = serde_json::from_str(data)?;
        if event.get("error").is_some() {
            bail!("{}", error_message(data));
        }
        let choice = event.pointer("/choices/0");
        if let Some(delta) = choice
            .and_then(|c| c.pointer("/delta/content"))
            .and_then(Value::as_str)
        {
            on_delta(delta);
        }
        match choice
            .and_then(|c| c.get("finish_reason"))
            .and_then(Value::as_str)
        {
            None => Ok(Flow::More),
            Some("stop") => Ok(Flow::Done),
            Some(reason) => bail!("the answer was cut off ({reason})"),
        }
    })
    .await
}

pub async fn models(http: &reqwest::Client, base: &str, key: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct List {
        data: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    let mut request = http.get(format!("{base}/models"));
    if !key.is_empty() {
        request = request.bearer_auth(key);
    }
    let list: List = check(request.send().await?).await?.json().await?;
    Ok(list.data.into_iter().map(|m| m.id).collect())
}
