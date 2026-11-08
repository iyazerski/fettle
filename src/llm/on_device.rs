//! Apple's on-device model, through the Swift bridge in `swift/on_device.swift`:
//! free and private, on Macs with Apple Intelligence.

use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt;

use anyhow::{Result, anyhow, bail};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use super::Ask;

unsafe extern "C" {
    safe fn fettle_on_device_status() -> i32;
    fn fettle_on_device_stream(
        instructions: *const c_char,
        prompt: *const c_char,
        context: *mut c_void,
        on_chunk: extern "C" fn(*mut c_void, *const c_char),
        on_done: extern "C" fn(*mut c_void, i32, *const c_char),
    ) -> *mut c_void;
    fn fettle_on_device_end(job: *mut c_void);
}

/// Whether the on-device model can run, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Ready,
    /// macOS before 26, which has no on-device model.
    OldMacOS,
    /// A Mac that can't run Apple Intelligence.
    NotEligible,
    /// Apple Intelligence is off in System Settings.
    TurnedOff,
    /// Still downloading, or blocked by a setting such as Siri's language.
    NotReady,
}

/// Return whether the on-device model can run now, from the bridge's status
/// code (see `swift/on_device.swift`).
pub fn availability() -> Availability {
    match fettle_on_device_status() {
        0 => Availability::Ready,
        1 => Availability::OldMacOS,
        2 => Availability::NotEligible,
        3 => Availability::TurnedOff,
        _ => Availability::NotReady,
    }
}

/// Why the on-device model gave no answer, for the failures Fettle explains in
/// the window's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    TooLong,
    Language,
    Declined,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refusal::TooLong => "the text is too long for the on-device model",
            Refusal::Language => "the on-device model doesn't support this language",
            Refusal::Declined => "the on-device model declined the text",
        })
    }
}

impl std::error::Error for Refusal {}

/// What the bridge reports while it answers.
enum Event {
    Chunk(String),
    Done(Result<()>),
}

type Events = UnboundedSender<Event>;

/// A running answer; dropping it cancels the answer if it still runs.
struct Job(*mut c_void);

// SAFETY: the job is a retained Swift object whose only use, ending it, is
// safe from any thread.
unsafe impl Send for Job {}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `fettle_on_device_stream` and is ended once, here.
        unsafe { fettle_on_device_end(self.0) };
    }
}

/// Stream the answer to `ask`, calling `on_delta` with each piece.
pub async fn stream(ask: &Ask, mut on_delta: impl FnMut(&str)) -> Result<()> {
    let instructions = CString::new(ask.system.as_str())?;
    let prompt = CString::new(ask.text.as_str())?;
    let (events, mut received) = unbounded_channel();
    let context = Box::into_raw(Box::new(events)).cast::<c_void>();
    // SAFETY: Swift copies both strings before returning, and calls `on_done`,
    // which frees `context`, exactly once after its last `on_chunk`.
    let _job = Job(unsafe {
        fettle_on_device_stream(
            instructions.as_ptr(),
            prompt.as_ptr(),
            context,
            on_chunk,
            on_done,
        )
    });
    while let Some(event) = received.recv().await {
        match event {
            Event::Chunk(piece) => on_delta(&piece),
            Event::Done(result) => return result,
        }
    }
    bail!("the on-device model stopped without an answer")
}

extern "C" fn on_chunk(context: *mut c_void, piece: *const c_char) {
    // SAFETY: `context` is the sender `stream` boxed, alive until `on_done`;
    // `piece` is a NUL-terminated string valid for this call.
    let (events, piece) = unsafe { (&*context.cast::<Events>(), CStr::from_ptr(piece)) };
    // A closed channel means the answer was cancelled; the piece is not needed.
    let _ = events.send(Event::Chunk(piece.to_string_lossy().into_owned()));
}

extern "C" fn on_done(context: *mut c_void, code: i32, message: *const c_char) {
    // SAFETY: Swift calls this once, last, so the box `stream` made is freed
    // exactly once; `message` is a NUL-terminated string valid for this call.
    let (events, message) = unsafe {
        (
            Box::from_raw(context.cast::<Events>()),
            CStr::from_ptr(message),
        )
    };
    let _ = events.send(Event::Done(ended(code, &message.to_string_lossy())));
}

/// Return how an answer ended, from `OnDone`'s code (see `swift/on_device.swift`)
/// and message.
fn ended(code: i32, message: &str) -> Result<()> {
    match code {
        0 => Ok(()),
        2 => Err(Refusal::TooLong.into()),
        3 => Err(Refusal::Language.into()),
        4 => Err(Refusal::Declined.into()),
        _ => Err(anyhow!("{message}")),
    }
}
