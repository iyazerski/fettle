//! Server-sent events, as both APIs stream their answers.

use anyhow::{Result, bail};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;

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
    let mut events = response.bytes_stream().eventsource();
    while let Some(event) = events.next().await {
        if let Flow::Done = on_event(&event?.data)? {
            return Ok(());
        }
    }
    bail!("the answer stopped early")
}
