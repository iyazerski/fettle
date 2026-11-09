//! Dictating into the main page's text: listen while the waveform shows the
//! microphone's loudness, then add what was heard to the text, ready for any
//! action. The model quietly turns the raw speech into written text first.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::draft::hyphens;
use super::state::State;
use crate::dictation::{self, Heard, Listening, Problem};
use crate::llm::{Ask, Backend};
use crate::prompts;

/// How many recent loudness readings the waveform shows.
pub const LEVELS_KEPT: usize = 60;

/// Dictation into the text.
pub enum Dictation {
    /// Waiting for macOS to allow speech recognition and the microphone.
    Asking,
    Listening(Recording),
    /// Done listening; the model is turning what was heard into written text.
    Writing(Recording),
}

/// A dictation session, and what the window shows of it.
pub struct Recording {
    /// Which session this is, so a late report of an ended one is dropped.
    id: u64,
    session: Listening,
    started: Instant,
    /// When listening ended, which stops the clock.
    ended: Option<Instant>,
    /// What's heard so far.
    heard: String,
    /// Recent loudness, 0 to 1, oldest first.
    pub levels: VecDeque<f32>,
}

impl Recording {
    /// Return how long the session listened, or has so far.
    pub fn duration(&self) -> Duration {
        self.ended.unwrap_or_else(Instant::now) - self.started
    }
}

impl State {
    /// Start dictating into the text, or stop.
    pub fn dictate(&mut self) {
        match &self.dictation {
            Some(Dictation::Asking | Dictation::Writing(_)) => {}
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
                    ended: None,
                    heard: String::new(),
                    levels: VecDeque::with_capacity(LEVELS_KEPT),
                }));
            }
            Err(problem) => self.dictation_failed(&problem),
        }
    }

    /// Take what session `id` reports. Once it ends, what it heard is written up.
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
                let Some(Dictation::Listening(mut recording)) = self.dictation.take() else {
                    return;
                };
                if !recording.heard.trim().is_empty() {
                    recording.ended = Some(Instant::now());
                    self.write_up(recording);
                }
            }
            Heard::Failed(why) => {
                self.dictation = None;
                self.dictation_failed(&Problem::Failed(why));
            }
        }
    }

    /// Have the model turn what `recording` heard into written text, then add
    /// it. Without a model, the words go in as heard.
    fn write_up(&mut self, recording: Recording) {
        let (backend, model) = match Backend::with_model(&self.settings) {
            Ok(chosen) => chosen,
            Err(_) => return self.add_dictated(&recording.heard),
        };
        let (id, ask) = (
            recording.id,
            Ask {
                system: prompts::dictation(),
                text: recording.heard.clone(),
                model,
                effort: self.settings.effort(),
            },
        );
        self.dictation = Some(Dictation::Writing(recording));
        let client = self.client.clone();
        self.client.spawn(async move {
            let result = client.answer(backend, &ask).await;
            client.app().update(move |s, _| s.written_up(id, result));
        });
    }

    /// Add the model's write-up of session `id`, or what it heard as is if the
    /// model failed: dictation is input, so a failed write-up isn't an error.
    fn written_up(&mut self, id: u64, result: anyhow::Result<String>) {
        // Hiding the window while the model worked gave up on the words.
        let Some(Dictation::Writing(recording)) = self
            .dictation
            .take_if(|d| matches!(d, Dictation::Writing(recording) if recording.id == id))
        else {
            return;
        };
        let heard = recording.heard;
        match result {
            // Models add dashes speech never has.
            Ok(text) if !text.trim().is_empty() => {
                self.add_dictated(&hyphens(&heard, text.trim()));
            }
            Ok(_) => self.add_dictated(&heard),
            Err(e) => {
                eprintln!("writing up dictation: {e:#}");
                self.add_dictated(&heard);
            }
        }
    }

    /// Put `words` after the text.
    fn add_dictated(&mut self, words: &str) {
        let text = dictation::join(&self.draft.source.text, words);
        self.draft.source.replace_text(text);
    }

    /// Say under the text why dictation can't run.
    fn dictation_failed(&mut self, problem: &Problem) {
        self.draft.notice = Some(self.t().dictation_problem(problem));
    }
}
