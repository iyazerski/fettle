//! Underlining mistakes as the user types in other apps: read the focused
//! text field whenever the app reports a change, and on a timer for apps that
//! don't; once the user pauses, ask the model to fix each paragraph they
//! edited, as the Fix action does; and underline what its fix changes. A click
//! on an underline opens the paragraph in the window with that fix.
//!
//! The reading runs on a thread of its own ([`worker`]), as other apps can
//! be slow to answer; the window keeps the underlines and the clicks on them.

mod worker;

use objc2_core_foundation::CGRect;
use rune::AppHandle;

use self::worker::{Command, Handle};
use super::pending::Pending;
use super::state::State;
use crate::diff::Piece;
use crate::underlines::{Clicks, Line, Underlines};

/// The window's side of the checker.
#[derive(Default)]
pub struct Checker {
    running: Option<Running>,
    underlines: Underlines,
    /// The last checker's number, so underlines from a stopped one are ignored.
    started: u64,
}

/// What runs while underlining is on.
struct Running {
    worker: Handle,
    _clicks: Option<Clicks>,
}

/// The model's fix of a paragraph, as the Fix action shows it.
#[derive(Debug, Clone)]
pub struct Fix {
    pub text: String,
    /// The word diff from the paragraph to `text`.
    pub changes: Vec<Piece>,
}

/// A paragraph to ask the model to fix, as check `id`.
#[derive(Debug)]
pub struct Check {
    pub id: u64,
    /// The paragraph as the field holds it, which its fix is kept by.
    pub units: Vec<u16>,
    pub text: String,
}

/// A paragraph opened from an underline, to fix and paste back over.
#[derive(Debug)]
pub struct Target {
    /// The paragraph as the field holds it.
    pub units: Vec<u16>,
    pub text: String,
    /// The underlined word clicked, in points from the primary screen's top-left.
    pub word: CGRect,
}

impl Checker {
    /// Start reading the focused field and watching clicks, unless already started.
    pub fn start(&mut self, app: &AppHandle<State>) {
        if self.running.is_some() {
            return;
        }
        self.started += 1;
        let Some(worker) = worker::spawn(app.clone(), self.started) else {
            return;
        };
        let clicks = app.clone();
        self.running = Some(Running {
            worker,
            _clicks: Clicks::watch(move |at| clicks.update(move |s, _| s.underline_clicked(at))),
        });
    }

    pub fn stop(&mut self) {
        self.running = None;
        self.underlines.hide();
    }

    /// Underline `lines`, which checker `id` found, unless it was stopped.
    pub fn show(&mut self, id: u64, lines: Vec<Line>) {
        if self.running.is_some() && id == self.started {
            self.underlines.show(lines);
        }
    }

    /// Open the paragraph with the underline at `point`, in AppKit's
    /// coordinates, if there's one there.
    pub fn clicked(&self, point: (f64, f64)) {
        if let Some(line) = self.underlines.hit(point) {
            self.send(Command::Open(line));
        }
    }

    /// Keep `request`, which asks the model about check `id`, until it
    /// finishes or the field's session ends.
    pub fn asking(&self, id: u64, request: Pending) {
        self.send(Command::Asking(id, request));
    }

    /// Take the model's `fix` of paragraph `units`, asked as check `id`, or
    /// `None` if the request failed.
    pub fn finished(&self, id: u64, units: Vec<u16>, fix: Option<Fix>) {
        self.send(Command::Finished { id, units, fix });
    }

    /// Leave the edits of the field that asked for check `id` unchecked:
    /// there's no model to ask.
    pub fn skip(&self, id: u64) {
        self.send(Command::Skip(id));
    }

    /// Forget the fixes known: they came from another model.
    pub fn forget(&self) {
        self.send(Command::Forget);
    }

    /// Keep the model's `fix` of `target`'s paragraph, so it isn't sent again.
    pub fn learn(&self, target: &Target, fix: Fix) {
        self.send(Command::Learn {
            units: target.units.clone(),
            fix,
        });
    }

    /// Select `target`'s paragraph in its field, so a paste replaces it.
    /// The window hears back whether it could, unless this returns false:
    /// underlining is off, and nothing can select it.
    pub fn select(&self, target: &Target) -> bool {
        self.send(Command::Select(target.units.clone()))
    }

    /// Send `command` to the checker thread; false if underlining is off.
    fn send(&self, command: Command) -> bool {
        let Some(running) = &self.running else {
            return false;
        };
        running.worker.send(command);
        true
    }
}
