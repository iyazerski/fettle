//! Dictating into the main page's text: listen while the waveform shows the
//! microphone's loudness, then add what was heard to the text and clean it up.

use std::collections::VecDeque;
use std::time::Instant;

use super::state::State;
use crate::dictation::{self, Heard, Listening, Problem};
use crate::prompts::Action;

/// How many recent loudness readings the waveform shows.
pub const LEVELS_KEPT: usize = 60;

/// Dictation into the text.
pub enum Dictation {
    /// Waiting for macOS to allow speech recognition and the microphone.
    Asking,
    Listening(Recording),
}

/// A dictation session that's listening.
pub struct Recording {
    /// Which session this is, so a late report of an ended one is dropped.
    id: u64,
    session: Listening,
    pub started: Instant,
    /// What's heard so far, added to the text once listening ends.
    heard: String,
    /// Recent loudness, 0 to 1, oldest first.
    pub levels: VecDeque<f32>,
}

impl State {
    /// Start dictating into the text, or stop and clean up what was heard.
    pub fn dictate(&mut self) {
        match &self.dictation {
            Some(Dictation::Asking) => {}
            Some(Dictation::Listening(recording)) => recording.session.stop(),
            None => {
                self.draft.notice = None;
                self.dictation = Some(Dictation::Asking);
                let app = self.app.clone();
                dictation::authorize(move |allowed| {
                    app.update(move |s, _| s.dictation_allowed(allowed));
                });
            }
        }
    }

    /// Start listening if macOS `allowed` it and the window still waits for it.
    fn dictation_allowed(&mut self, allowed: Result<(), Problem>) {
        // Hiding the window while macOS asked gave up on dictating.
        if !matches!(self.dictation, Some(Dictation::Asking)) {
            return;
        }
        self.dictation = None;
        if let Err(problem) = allowed {
            return self.dictation_failed(&problem);
        }
        let id = self.next_dictation;
        self.next_dictation += 1;
        let app = self.app.clone();
        let heard = move |heard: Heard| app.update(move |s, _| s.heard(id, heard));
        match Listening::start(heard) {
            Ok(session) => {
                self.dictation = Some(Dictation::Listening(Recording {
                    id,
                    session,
                    started: Instant::now(),
                    heard: String::new(),
                    levels: VecDeque::with_capacity(LEVELS_KEPT),
                }));
            }
            Err(problem) => self.dictation_failed(&problem),
        }
    }

    /// Take what session `id` reports. Once it ends, what it heard goes after
    /// the text, and the model cleans the text up.
    fn heard(&mut self, id: u64, heard: Heard) {
        let Some(Dictation::Listening(recording)) = &mut self.dictation else {
            return;
        };
        if recording.id != id {
            return;
        }
        match heard {
            Heard::Words(words) => recording.heard = words,
            Heard::Level(level) => {
                if recording.levels.len() == LEVELS_KEPT {
                    recording.levels.pop_front();
                }
                recording.levels.push_back(level);
            }
            Heard::Done => {
                let words = std::mem::take(&mut recording.heard);
                self.dictation = None;
                if words.trim().is_empty() {
                    return;
                }
                let text = dictation::join(&self.draft.source.text, &words);
                self.draft.source.replace_text(text);
                self.start(Action::Dictation);
            }
            Heard::Failed(why) => {
                self.dictation = None;
                self.dictation_failed(&Problem::Failed(why));
            }
        }
    }

    /// Say under the text why dictation can't run.
    fn dictation_failed(&mut self, problem: &Problem) {
        self.draft.notice = Some(self.t().dictation_problem(problem));
    }
}
