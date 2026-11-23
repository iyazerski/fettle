use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::pending::Ticket;
use super::state::State;
use crate::dictation::{self, Heard, Listening, Problem};
use crate::hyphens;
use crate::llm::{Ask, Backend};
use crate::prompts;

pub const LEVELS_KEPT: usize = 60;

pub enum Dictation {
    Asking,
    Listening(Recording),
    Writing(WriteUp),
}

pub struct Recording {
    id: Ticket,
    session: Listening,
    started: Instant,
    heard: String,
    pub levels: VecDeque<f32>,
}

pub struct WriteUp {
    id: Ticket,
    pub levels: VecDeque<f32>,
    pub took: Duration,
}

impl Recording {
    pub fn duration(&self) -> Duration {
        self.started.elapsed()
    }

    fn into_write_up(self) -> (WriteUp, String) {
        let write_up = WriteUp {
            took: self.duration(),
            id: self.id,
            levels: self.levels,
        };
        (write_up, self.heard)
    }
}

impl State {
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

    fn dictation_allowed(&mut self, allowed: Result<(), Problem>) {
        // Hiding the window while macOS asked gave up on dictating.
        if !matches!(self.dictation, Some(Dictation::Asking)) {
            return;
        }
        self.dictation = None;
        if let Err(problem) = allowed {
            return self.dictation_failed(&problem);
        }
        let id = Ticket::new();
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

    fn heard(&mut self, id: Ticket, heard: Heard) {
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
                let Some(Dictation::Listening(recording)) = self.dictation.take() else {
                    return;
                };
                let (write_up, heard) = recording.into_write_up();
                if !heard.trim().is_empty() {
                    self.write_up(write_up, heard);
                }
            }
            Heard::Failed(why) => {
                let Some(Dictation::Listening(recording)) = self.dictation.take() else {
                    return;
                };
                if !recording.heard.trim().is_empty() {
                    self.add_dictated(&recording.heard);
                }
                self.dictation_failed(&Problem::Failed(why));
            }
        }
    }

    fn write_up(&mut self, write_up: WriteUp, heard: String) {
        let Ok((backend, model)) = Backend::with_model(&self.settings) else {
            return self.add_dictated(&heard);
        };
        let id = write_up.id;
        let ask = Ask {
            system: prompts::dictation(),
            text: heard,
            model,
            effort: self.settings.effort(),
        };
        self.dictation = Some(Dictation::Writing(write_up));
        let client = self.client.clone();
        self.client.spawn(async move {
            let result = client.answer(backend, &ask).await;
            let heard = ask.text;
            client
                .app()
                .update(move |s, _| s.written_up(id, &heard, result));
        });
    }

    /// Keep the raw words if cleanup fails: dictation is input, not a model action.
    fn written_up(&mut self, id: Ticket, heard: &str, result: anyhow::Result<String>) {
        // Hiding the window while the model worked gave up on the words.
        if self
            .dictation
            .take_if(|d| matches!(d, Dictation::Writing(write_up) if write_up.id == id))
            .is_none()
        {
            return;
        }
        match result {
            Ok(text) if !text.trim().is_empty() => {
                self.add_dictated(&hyphens::restore(heard, text.trim()));
            }
            Ok(_) => self.add_dictated(heard),
            Err(e) => {
                eprintln!("writing up dictation: {e:#}");
                self.add_dictated(heard);
            }
        }
    }

    fn add_dictated(&mut self, words: &str) {
        let text = dictation::join(&self.draft.source.text, words);
        self.draft.source.replace_text(text);
    }

    fn dictation_failed(&mut self, problem: &Problem) {
        self.draft.notice = Some(self.t().dictation_problem(problem));
    }
}
