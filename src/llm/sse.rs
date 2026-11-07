//! Server-sent events, as both APIs stream them: `data:` lines, one event per
//! blank-line-separated block.

use anyhow::{Result, bail};
use futures_util::StreamExt;

/// Splits a byte stream into the `data` of each event.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    data: Vec<String>,
}

impl Decoder {
    /// Feed `bytes` and return the events they complete.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    events.push(self.data.join("\n"));
                    self.data.clear();
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                self.data
                    .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
            }
        }
        events
    }

    /// The event the stream ended in the middle of, if any.
    pub fn finish(&mut self) -> Option<String> {
        let tail = self.push(b"\n\n");
        tail.into_iter().next()
    }
}

/// What to do after an event.
pub enum Flow {
    More,
    Done,
}

/// Run `on_event` on the data of each event in `response`'s body, until one
/// says the answer is done.
///
/// # Errors
///
/// Fails if reading the body or `on_event` fails, or if the body ends before
/// an event said the answer was done: the answer would be cut short.
pub async fn each(
    response: reqwest::Response,
    mut on_event: impl FnMut(&str) -> Result<Flow>,
) -> Result<()> {
    let mut decoder = Decoder::default();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        for event in decoder.push(&chunk?) {
            if let Flow::Done = on_event(&event)? {
                return Ok(());
            }
        }
    }
    if let Some(event) = decoder.finish()
        && let Flow::Done = on_event(&event)?
    {
        return Ok(());
    }
    bail!("the answer stopped early")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_split_across_chunks_and_lines() {
        let mut d = Decoder::default();
        assert!(d.push(b"event: x\ndata: {\"a\"").is_empty());
        assert_eq!(
            d.push(b":1}\r\n\r\ndata: one\ndata:two\n\n: ping\n\n"),
            ["{\"a\":1}", "one\ntwo"]
        );
        assert!(d.push(b"data: [DONE]").is_empty());
        assert_eq!(d.finish().as_deref(), Some("[DONE]"));
        assert_eq!(d.finish(), None);
    }
}
