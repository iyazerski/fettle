//! A ChatGPT plan as the model: the browser sign-in Codex uses (OAuth with
//! PKCE and a callback on localhost:1455), token refresh, and the Codex
//! Responses endpoint.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use super::sse::{self, Flow};
use super::{Ask, check, error_message};
use crate::settings::ChatGptAuth;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const SCOPE: &str = "openid profile email offline_access";
const CODEX_URL: &str = "https://chatgpt.com/backend-api/codex";
/// The JWT claim with the ChatGPT account.
const AUTH_CLAIM: &str = "https://api.openai.com/auth";
/// Refresh this long before the access token expires.
const REFRESH_EARLY: i64 = 300;
/// How long the browser sign-in may take.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// Sign in through the browser: listen for the callback, have `open` show
/// the sign-in page, and trade the code it returns for tokens.
pub async fn login(http: &reqwest::Client, open: impl FnOnce(&str)) -> Result<ChatGptAuth> {
    let verifier = random_token(32)?;
    let challenge = URL_SAFE_NO_PAD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        verifier.as_bytes(),
    ));
    let state = random_token(16)?;
    let mut url = url::Url::parse(AUTHORIZE_URL)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "fettle");
    let listener = TcpListener::bind(("127.0.0.1", 1455)).await.context(
        "port 1455 is busy: close other apps that are signing in to OpenAI and try again",
    )?;
    open(url.as_str());
    let code = tokio::time::timeout(LOGIN_TIMEOUT, callback(&listener, &state))
        .await
        .map_err(|_| anyhow!("the sign-in timed out"))??;
    drop(listener);
    let tokens = token(
        http,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", REDIRECT_URI),
        ],
    )
    .await?;
    tokens.into_auth(None)
}

/// `auth` refreshed if it's about to expire, else `None`.
pub async fn fresh(http: &reqwest::Client, auth: &ChatGptAuth) -> Result<Option<ChatGptAuth>> {
    if auth.expires_at - now() > REFRESH_EARLY {
        return Ok(None);
    }
    let tokens = token(
        http,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &auth.refresh),
            ("client_id", CLIENT_ID),
        ],
    )
    .await
    .context("the ChatGPT sign-in expired: sign in again in Settings")?;
    tokens.into_auth(Some(auth)).map(Some)
}

pub async fn stream(
    http: &reqwest::Client,
    auth: &ChatGptAuth,
    ask: &Ask,
    mut on_delta: impl FnMut(&str),
) -> Result<()> {
    let mut body = json!({
        "model": ask.model,
        "store": false,
        "stream": true,
        "instructions": ask.system,
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": ask.text }],
        }],
        "text": { "verbosity": "low" },
    });
    if let Some(effort) = ask.effort {
        body["reasoning"] = json!({ "effort": effort });
    }
    let request = http
        .post(format!("{CODEX_URL}/responses"))
        .bearer_auth(&auth.access)
        .header("chatgpt-account-id", &auth.account_id)
        .header("originator", "fettle")
        .header("OpenAI-Beta", "responses=experimental")
        .header("accept", "text/event-stream")
        .json(&body);
    let response = check(request.send().await?).await?;
    sse::each(response, |data| {
        let event: Value = serde_json::from_str(data)?;
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    on_delta(delta);
                }
                Ok(Flow::More)
            }
            "response.completed" | "response.done" => Ok(Flow::Done),
            "response.incomplete" => {
                let reason = event
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                bail!("the answer was cut off ({reason})")
            }
            "response.failed" | "error" => bail!("{}", failure(&event)),
            _ => Ok(Flow::More),
        }
    })
    .await
}

/// The models the plan offers.
pub async fn models(http: &reqwest::Client, auth: &ChatGptAuth) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct List {
        models: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        slug: String,
        visibility: Option<String>,
    }
    let request = http
        .get(format!("{CODEX_URL}/models"))
        // The list leaves out models that need a newer Codex than this.
        .query(&[("client_version", "1.0.0")])
        .bearer_auth(&auth.access)
        .header("chatgpt-account-id", &auth.account_id)
        .header("originator", "fettle");
    let list: List = check(request.send().await?).await?.json().await?;
    Ok(list
        .models
        .into_iter()
        .filter(|m| m.visibility.as_deref() != Some("hide"))
        .map(|m| m.slug)
        .collect())
}

/// The message of a `response.failed` or `error` event.
fn failure(event: &Value) -> String {
    let message = event
        .pointer("/response/error/message")
        .or_else(|| event.get("message"))
        .and_then(Value::as_str);
    message.map_or_else(|| error_message(&event.to_string()), str::to_owned)
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: i64,
}

impl Tokens {
    /// The sign-in these tokens make, keeping what a refresh leaves out from `old`.
    fn into_auth(self, old: Option<&ChatGptAuth>) -> Result<ChatGptAuth> {
        let account_id = claims(&self.access_token)
            .and_then(|c| {
                c.get(AUTH_CLAIM)?
                    .get("chatgpt_account_id")?
                    .as_str()
                    .map(str::to_owned)
            })
            .context("the sign-in has no ChatGPT account: use an account with a ChatGPT plan")?;
        let email = self
            .id_token
            .as_deref()
            .and_then(claims)
            .and_then(|c| c.get("email")?.as_str().map(str::to_owned))
            .or_else(|| old.and_then(|o| o.email.clone()));
        let refresh = self
            .refresh_token
            .or_else(|| old.map(|o| o.refresh.clone()))
            .context("the sign-in returned no refresh token")?;
        Ok(ChatGptAuth {
            access: self.access_token,
            refresh,
            expires_at: now() + self.expires_in,
            account_id,
            email,
        })
    }
}

async fn token(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<Tokens> {
    let response = http.post(TOKEN_URL).form(form).send().await?;
    Ok(check(response).await?.json().await?)
}

/// Serve the browser until it brings the sign-in back, and return its code.
/// Each connection is read on its own: browsers open idle ones ahead of use.
async fn callback(listener: &TcpListener, state: &str) -> Result<String> {
    let mut heads = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (socket, _) = accepted?;
                heads.spawn(read_head(socket));
            }
            Some(read) = heads.join_next() => {
                // A connection that broke off isn't the sign-in.
                let Ok(Ok((mut socket, target))) = read else {
                    continue;
                };
                match url::Url::parse(&format!("http://localhost{target}")) {
                    Ok(url) if url.path() == "/auth/callback" => {
                        return finish_sign_in(socket, &url, state).await;
                    }
                    _ => {
                        let _ = socket.write_all(NOT_FOUND).await;
                    }
                }
            }
        }
    }
}

const NOT_FOUND: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// Read a request's head from `socket`, and return the socket with the request's target.
async fn read_head(mut socket: TcpStream) -> std::io::Result<(TcpStream, String)> {
    let mut head = Vec::new();
    let mut buf = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(&buf[..n]);
    }
    let head = String::from_utf8_lossy(&head);
    let target = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
    Ok((socket, target))
}

/// Answer the browser's sign-in callback at `url` with a page, and return its code.
async fn finish_sign_in(mut socket: TcpStream, url: &url::Url, state: &str) -> Result<String> {
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    let result = match (param("code"), param("error")) {
        _ if param("state").as_deref() != Some(state) => {
            Err(anyhow!("the sign-in came back for another request"))
        }
        (Some(code), None) => Ok(code),
        (_, error) => Err(anyhow!(
            "the sign-in failed: {}",
            param("error_description")
                .or(error)
                .unwrap_or_else(|| "no code".into())
        )),
    };
    let page = match &result {
        Ok(_) => "Signed in to Fettle. You can close this tab.".to_owned(),
        Err(e) => format!("Fettle couldn't sign in: {e}"),
    };
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>Fettle</title>\
         <body style=\"font:16px -apple-system;padding:48px;text-align:center\">{page}</body>"
    );
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    socket.write_all(reply.as_bytes()).await?;
    let _ = socket.shutdown().await;
    result
}

/// A JWT's claims, unverified: the token came straight from OpenAI over TLS.
fn claims(jwt: &str) -> Option<Value> {
    let payload = jwt.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?).ok()
}

fn random_token(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| anyhow!("no system randomness"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::callback;

    #[tokio::test]
    async fn sign_in_callback_gets_past_idle_and_other_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let browser = tokio::spawn(async move {
            // Browsers open connections before they have a request for them.
            let idle = TcpStream::connect(addr).await.unwrap();
            for target in ["/favicon.ico", "/auth/callback?code=c1&state=s1"] {
                let mut socket = TcpStream::connect(addr).await.unwrap();
                let request = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
                socket.write_all(request.as_bytes()).await.unwrap();
                let mut reply = String::new();
                socket.read_to_string(&mut reply).await.unwrap();
            }
            idle
        });
        let code = tokio::time::timeout(Duration::from_secs(5), callback(&listener, "s1"))
            .await
            .expect("the callback should not wait on the idle connection");
        assert_eq!(code.unwrap(), "c1");
        browser.await.unwrap();
    }
}
