//! The main page's text and the answer about it: which run is current, and
//! what a run's late pieces may still change.

use rune::TextEdit;
use tokio::task::JoinHandle;

use crate::diff::{self, Piece};
use crate::prompts::Action;

/// The text to work on and the answer about it.
pub struct Draft {
    /// The selection, editable.
    pub source: TextEdit,
    pub answer: Option<Answer>,
    /// Show the fixed text as a word diff rather than plain text.
    pub show_changes: bool,
    /// A one-line note under the text: nothing selected, a save error.
    pub notice: Option<String>,
    /// Counts runs, so a late piece of an old answer is dropped.
    run: u64,
}

pub struct Answer {
    pub action: Action,
    pub status: Status,
}

pub enum Status {
    /// Streaming run `run`'s answer into `text`.
    Running {
        run: u64,
        job: JoinHandle<()>,
        text: String,
    },
    /// The user stopped the run.
    Stopped,
    Done {
        text: String,
        /// For Fix: the word diff from the original to `text`.
        changes: Vec<Piece>,
        copied: bool,
    },
    Failed(String),
}

impl Draft {
    pub fn new() -> Self {
        Self {
            source: TextEdit::new(""),
            answer: None,
            show_changes: true,
            notice: None,
            run: 0,
        }
    }

    /// Start over on `text`, with `notice` under it.
    pub fn reset(&mut self, text: String, notice: Option<String>) {
        self.answer = None;
        self.notice = notice;
        self.source.replace_text(text);
    }

    /// Stop the running answer, if any, and return the number of the next run.
    pub fn cancel(&mut self) -> u64 {
        self.run += 1;
        if let Some(answer) = self.answer.as_mut()
            && let Status::Running { job, .. } = &answer.status
        {
            job.abort();
            answer.status = Status::Stopped;
        }
        self.run
    }

    /// Show that `action` failed with `message`.
    pub fn fail(&mut self, action: Action, message: String) {
        self.answer = Some(Answer {
            action,
            status: Status::Failed(message),
        });
    }

    /// Show `status` as `action`'s answer, known without a run.
    pub fn answered(&mut self, action: Action, status: Status) {
        self.answer = Some(Answer { action, status });
    }

    /// Show `action`'s run `run`, which `job` streams in.
    pub fn running(&mut self, action: Action, run: u64, job: JoinHandle<()>) {
        self.answer = Some(Answer {
            action,
            status: Status::Running {
                run,
                job,
                text: String::new(),
            },
        });
    }

    /// Add `delta` to run `run`'s answer, if it's still the one running.
    pub fn delta(&mut self, run: u64, delta: &str) {
        if let Some(text) = self.running_text(run) {
            text.push_str(delta);
        }
    }

    /// End run `run` with `status`, if it's still the one running.
    pub fn finished(&mut self, run: u64, status: Status) {
        if self.running_text(run).is_none() {
            return;
        }
        if let Some(answer) = self.answer.as_mut() {
            answer.status = status;
        }
    }

    fn running_text(&mut self, run: u64) -> Option<&mut String> {
        match &mut self.answer.as_mut()?.status {
            Status::Running {
                run: current, text, ..
            } if *current == run => Some(text),
            _ => None,
        }
    }

    /// Return the finished answer's text.
    pub fn result(&self) -> Option<&str> {
        match &self.answer.as_ref()?.status {
            Status::Done { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Mark the finished answer as copied.
    pub fn copied(&mut self) {
        if let Some(Answer {
            status: Status::Done { copied, .. },
            ..
        }) = self.answer.as_mut()
        {
            *copied = true;
        }
    }

    /// Whether the answer is a translation, which a new language redoes.
    pub fn translated(&self) -> bool {
        self.answer
            .as_ref()
            .is_some_and(|a| a.action == Action::Translate)
    }
}

/// Dashes models put in place of a hyphen, often dropping its spaces too.
const DASHES: [char; 2] = ['—', '–'];

/// Return the status of an `action` on `original` whose answer streamed in
/// full as `text`: done, with Fix's word diff, unless the model returned
/// nothing, which fails with `empty`. Dashes `original` doesn't use become
/// hyphens again, as models add them even when told not to.
pub fn complete(action: Action, original: &str, text: &str, empty: &str) -> Status {
    if text.trim().is_empty() {
        return Status::Failed(empty.into());
    }
    // Models often add or drop a final newline; keep the selection's.
    let end = &original[original.trim_end().len()..];
    let text = format!("{}{end}", hyphens(original, text.trim_end()));
    let changes = match action {
        Action::Fix => diff::words(original, &text),
        Action::Translate | Action::Rewrite(_) => Vec::new(),
    };
    Status::Done {
        text,
        changes,
        copied: false,
    }
}

/// Replace the [`DASHES`] in `text` that `original` lacks with spaced
/// hyphens, or bare ones between digits, as in ranges.
pub fn hyphens(original: &str, text: &str) -> String {
    let dashes: Vec<char> = DASHES
        .into_iter()
        .filter(|d| !original.contains(*d))
        .collect();
    if !text.contains(dashes.as_slice()) {
        return text.to_owned();
    }
    let blank = |c: char| c == ' ' || c == '\t' || c == '\u{a0}';
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if !dashes.contains(&c) {
            out.push(c);
            continue;
        }
        let range = out.ends_with(|c: char| c.is_ascii_digit())
            && chars.peek().is_some_and(char::is_ascii_digit);
        if range {
            out.push('-');
            continue;
        }
        out.truncate(out.trim_end_matches(blank).len());
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        out.push('-');
        while chars.next_if(|&c| blank(c)).is_some() {}
        if chars.peek().is_some_and(|&c| c != '\n') {
            out.push(' ');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_the_text_lacks_become_hyphens() {
        let cases = [
            ("xyz - zyz", "xyz—zyz", "xyz - zyz"),
            ("xyz - zyz", "xyz – zyz", "xyz - zyz"),
            ("a - b", "а — б", "а - б"),
            ("- one\n- two", "— one\n— two", "- one\n- two"),
            ("pages 10-20", "pages 10–20", "pages 10-20"),
            ("a — b", "a — b – c", "a — b - c"),
        ];
        for (original, answer, want) in cases {
            let got = match complete(Action::Fix, original, answer, "empty") {
                Status::Done { text, .. } => text,
                _ => String::new(),
            };
            assert_eq!(got, want, "{original:?} answered {answer:?}");
        }
    }

    fn text(draft: &Draft) -> Option<&str> {
        match &draft.answer.as_ref()?.status {
            Status::Running { text, .. } | Status::Done { text, .. } => Some(text),
            Status::Failed(message) => Some(message),
            Status::Stopped => Some("stopped"),
        }
    }

    #[tokio::test]
    async fn late_pieces_of_a_stopped_run_are_dropped() {
        let mut draft = Draft::new();
        let first = draft.cancel();
        draft.running(Action::Fix, first, tokio::spawn(async {}));
        draft.delta(first, "a");
        let second = draft.cancel();
        assert_eq!(text(&draft), Some("stopped"));

        draft.running(Action::Fix, second, tokio::spawn(async {}));
        draft.delta(first, "late");
        draft.finished(first, complete(Action::Fix, "x", "late", "empty"));
        draft.delta(second, "b");
        assert_eq!(text(&draft), Some("b"));
        draft.finished(second, complete(Action::Fix, "x\n", "b", "empty"));
        assert_eq!(draft.result(), Some("b\n"));
    }
}
