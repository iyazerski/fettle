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

fn ask(effort: Option<&'static str>) -> Ask {
    Ask {
        system: "Fix it.".into(),
        text: "teh text".into(),
        model: "m1".into(),
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
        .stream(&http().unwrap(), &ask(Some("low")), |d| answer.push_str(d))
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
    let (base, _request) = serve_once(
        "401 Unauthorized",
        "application/json",
        r#"{"error":{"message":"Incorrect API key provided"}}"#,
    )
    .await;
    let backend = Backend::Api {
        base,
        key: String::new(),
    };
    let error = backend
        .stream(&http().unwrap(), &ask(None), |_| {})
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Incorrect API key provided (401 Unauthorized)"
    );
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
