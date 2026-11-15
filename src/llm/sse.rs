use anyhow::{Result, bail};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;

#[derive(Debug)]
pub enum Flow {
    More,
    Done,
}

/// EOF without an explicit completion event is a truncated answer, not success.
pub async fn each(
    response: reqwest::Response,
    mut on_event: impl FnMut(&str) -> Result<Flow>,
) -> Result<()> {
    let mut events = response.bytes_stream().eventsource();
    while let Some(event) = events.next().await {
        if let Flow::Done = on_event(&event?.data)? {
            return Ok(());
        }
    }
    bail!("the answer stopped early")
}
