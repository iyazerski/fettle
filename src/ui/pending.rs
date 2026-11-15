//! Aborting a task can't retract queued UI updates; tickets reject stale results.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::task::JoinHandle;

use super::client::Client;

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

    pub fn answers(&self, ticket: Ticket) -> bool {
        self.ticket == ticket
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.job.abort();
    }
}
