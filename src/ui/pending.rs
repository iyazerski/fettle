//! Background work a page waits on. Aborting a task can't take back an
//! update it already queued for the window, so each result names the work it
//! answers, and only the current work's result lands.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::task::JoinHandle;

use super::client::Client;

/// Names one piece of background work, so its result can be matched to it.
/// Tickets are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket(u64);

impl Ticket {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Running work; dropping it aborts the work.
#[derive(Debug)]
pub struct Pending {
    ticket: Ticket,
    job: JoinHandle<()>,
}

impl Pending {
    /// Run the task `work` makes on `client`, giving it the ticket its
    /// result must carry.
    pub fn spawn<F>(client: &Client, work: impl FnOnce(Ticket) -> F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let ticket = Ticket::new();
        Self {
            ticket,
            job: client.spawn(work(ticket)),
        }
    }

    /// Whether a result with `ticket` answers this work.
    pub fn answers(&self, ticket: Ticket) -> bool {
        self.ticket == ticket
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.job.abort();
    }
}
