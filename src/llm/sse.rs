//! Server-sent events, as both APIs stream them: `data:` lines, one event per
//! blank-line-separated block.

use anyhow::{Result, bail};
use futures_util::StreamExt;

/// Splits a byte stream into the `data` of each event.
#[derive(Default)]
pub struct Decoder {
    /// Bytes after the last complete line.
    buf: Vec<u8>,
    /// The current event's `data` lines, joined; `None` before its first one.
    data: Option<String>,
}

impl Decoder {
    /// Feed `bytes` and return the events they complete.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut start = 0;
        while let Some(len) = self.buf[start..].iter().position(|b| *b == b'\n') {
            let line = &self.buf[start..start + len];
            start += len + 1;
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                events.extend(self.data.take());
            } else if let Some(value) = line.strip_prefix(b"data:") {
                let value = String::from_utf8_lossy(value.strip_prefix(b" ").unwrap_or(value));
                match &mut self.data {
                    Some(data) => {
                        data.push('\n');
                        data.push_str(&value);
                    }
                    None => self.data = Some(value.into_owned()),
                }
            }
        }
        // Drop the lines read in one go, rather than shifting the buffer per line.
        self.buf.drain(..start);
        events
    }

    /// The event the stream ended in the middle of, if any.
    pub fn finish(&mut self) -> Option<String> {
        let tail = self.push(b"\n\n");
        tail.into_iter().next()
    }
}

/// What to do after an event.
#[derive(Debug)]
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
