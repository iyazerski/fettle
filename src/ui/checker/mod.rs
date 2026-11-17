mod worker;

use std::time::Duration;

use objc2_core_foundation::CGRect;
use rune::AppHandle;

use self::worker::{Command, Handle};
use super::pending::{Pending, Ticket};
use super::state::State;
use crate::diff::{self, Piece};
use crate::underlines::{Clicks, Line, Underlines};

/// How long quitting waits for the checker to undo what it changed in other apps.
const QUIT_WAIT: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct Checker {
    running: Option<Running>,
    underlines: Underlines,
    /// The last checker's number, so underlines from a stopped one are ignored.
    started: u64,
}

struct Running {
    worker: Handle,
    _clicks: Option<Clicks>,
}

#[derive(Debug, Clone)]
pub struct Fix {
    pub text: String,
    pub changes: Vec<Piece>,
}

#[derive(Debug)]
pub struct Check {
    pub id: u64,
    /// The paragraph as the field holds it, which its fix is kept by.
    pub units: Vec<u16>,
    pub text: String,
}

#[derive(Debug)]
pub struct Target {
    pub opened: Ticket,
    pub text: String,
    /// The underlined word clicked, in points from the primary screen's top-left.
    pub word: CGRect,
}

impl Checker {
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
            _clicks: Clicks::watch(move |at| clicks.update(move |s, _| s.clicked_elsewhere(at))),
        });
    }

    pub fn stop(&mut self) {
        self.running = None;
        self.underlines.hide();
    }

    /// Wait for the worker's Accessibility cleanup, which process exit would skip.
    pub fn quit(&mut self) {
        if let Some(running) = self.running.take() {
            running.worker.stop_and_wait(QUIT_WAIT);
        }
        self.underlines.hide();
    }

    pub fn show(&mut self, id: u64, lines: Vec<Line>) {
        if self.running.is_some() && id == self.started {
            self.underlines.show(lines);
        }
    }

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

    pub fn finished(&self, id: u64, units: Vec<u16>, fix: Option<Fix>) {
        self.send(Command::Finished { id, units, fix });
    }

    pub fn skip(&self, id: u64) {
        self.send(Command::Skip(id));
    }

    pub fn forget(&self) {
        self.send(Command::Forget);
    }

    pub fn learn(&self, target: &Target, fix: Fix) {
        self.send(Command::LearnOpened {
            opened: target.opened,
            fix,
        });
    }

    /// Cache the accepted text as clean, including any changes the user declined.
    pub fn learn_fine(&self, paragraph: &str) {
        self.send(Command::Learn {
            units: paragraph.encode_utf16().collect(),
            fix: Fix {
                text: paragraph.to_owned(),
                changes: diff::words(paragraph, paragraph),
            },
        });
    }

    /// Request an asynchronous location check; false if underlining is off.
    pub fn locate(&self, target: &Target) -> bool {
        self.send(Command::Locate(target.opened))
    }

    /// Call once the window hides: the worker pastes with ⌘C and ⌘V in the field's app.
    pub fn replace(&self, text: String) {
        self.send(Command::Replace(text));
    }

    fn send(&self, command: Command) -> bool {
        let Some(running) = &self.running else {
            return false;
        };
        running.worker.send(command);
        true
    }
}
