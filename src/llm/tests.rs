use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

/// Serve one request with `status` and `body`, and hand back the request.
async fn serve_once(
    status: &str,
    content_type: &str,
    body: &str,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|v| v.parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if body.len() >= length {
                    break;
                }
            }
        }
        socket.write_all(reply.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (base, task)
}

fn ask(effort: Effort) -> Ask {
    Ask {
        system: "Fix it.".into(),
        text: "teh text".into(),
        model: Some("m1".into()),
        effort,
    }
}

#[tokio::test]
async fn api_streams_the_answer() {
    let events = concat!(
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"the \"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"text\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (base, request) = serve_once("200 OK", "text/event-stream", events).await;
    let backend = Backend::Api {
        base,
        key: "sk-1".into(),
    };
    let mut answer = String::new();
    backend
        .stream(&http().unwrap(), &ask(Effort::Low), |d| answer.push_str(d))
        .await
        .unwrap();
    assert_eq!(answer, "the text");

    let request = request.await.unwrap();
    assert!(request.starts_with("POST /v1/chat/completions "));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer sk-1")
    );
    let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["model"], "m1");
    assert_eq!(body["reasoning_effort"], "low");
    assert_eq!(body["messages"][0]["content"], "Fix it.");
    assert_eq!(body["messages"][1]["content"], "teh text");
}

#[tokio::test]
async fn api_errors_carry_the_servers_message() {
    let cases = [
        (
            r#"{"error":{"message":"Incorrect API key provided"}}"#,
            "Incorrect API key provided",
        ),
        (
            r#"{"error":{"code":"x"},"message":"No access"}"#,
            "No access",
        ),
        ("", "the server gave no reason"),
    ];
    for (body, want) in cases {
        let (base, _request) = serve_once("401 Unauthorized", "application/json", body).await;
        let backend = Backend::Api {
            base,
            key: String::new(),
        };
        let error = backend
            .stream(&http().unwrap(), &ask(Effort::None), |_| {})
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{want} (401 Unauthorized)"),
            "{body}"
        );
    }
}

#[tokio::test]
async fn api_lists_models_sorted() {
    let (base, _request) = serve_once(
        "200 OK",
        "application/json",
        r#"{"object":"list","data":[{"id":"zeta"},{"id":"alpha"}]}"#,
    )
    .await;
    let backend = Backend::Api {
        base,
        key: String::new(),
    };
    assert_eq!(
        backend.models(&http().unwrap()).await.unwrap(),
        ["alpha", "zeta"]
    );
}

#[tokio::test]
async fn api_answers_count_only_when_the_stream_says_they_are_done() {
    let delta = "data: {\"choices\":[{\"delta\":{\"content\":\"the text\"}}]}\n\n";
    let cases = [
        ("data: [DONE]\n\n", Ok(())),
        (
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            Ok(()),
        ),
        (
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            Err("the answer was cut off (length)"),
        ),
        ("", Err("the answer stopped early")),
    ];
    for (end, want) in cases {
        let events = format!("{delta}{end}");
        let (base, _request) = serve_once("200 OK", "text/event-stream", &events).await;
        let backend = Backend::Api {
            base,
            key: String::new(),
        };
        let got = backend
            .stream(&http().unwrap(), &ask(Effort::None), |_| {})
            .await
            .map_err(|e| e.to_string());
        assert_eq!(got, want.map_err(str::to_owned), "ending {end:?}");
    }
}

#[test]
fn requests_need_a_sign_in_an_api_url_and_a_model() {
    let mut settings = Settings {
        provider: Provider::ChatGpt,
        ..Settings::default()
    };
    assert_eq!(Backend::with_model(&settings).err(), Some(Missing::SignIn));
    settings.provider = Provider::Api;
    settings.api_base = " ".into();
    assert_eq!(Backend::with_model(&settings).err(), Some(Missing::ApiUrl));
    settings.api_base = "http://x/v1/".into();
    settings.api_model = " ".into();
    assert_eq!(Backend::with_model(&settings).err(), Some(Missing::Model));
    settings.api_model = " m1 ".into();
    let Ok((Backend::Api { base, .. }, model)) = Backend::with_model(&settings) else {
        panic!("an API with a URL and a model is ready");
    };
    assert_eq!(
        (base.as_str(), model.as_deref()),
        ("http://x/v1", Some("m1"))
    );
}
