//! The window's state and what changes it on the main page: showing the
//! selection, running an action, and moving the window. The settings page's
//! part is in [`settings_state`](super::settings_state).

use std::path::PathBuf;
use std::time::Duration;

use rune::{AppHandle, Phase, ShortcutId, TextEdit, Tx};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use super::settings_state::{Form, FormError, Update};
use super::view::WIDTH;
use crate::diff::{self, Piece};
use crate::i18n::{Locale, Strings, fill};
use crate::llm::{Ask, Backend, chatgpt};
use crate::mac;
use crate::prompts::{self, Action};
use crate::settings::Settings;

/// How far below the top of the screen the window opens.
const TOP_MARGIN: f32 = 120.0;
/// How long the window waits, hidden, before pasting, so the app the text
/// came from has its key window back.
const PASTE_DELAY: Duration = Duration::from_millis(120);
/// How long the window takes to grow for a dropdown.
const MENU_DELAY: Duration = Duration::from_millis(200);
/// How long typing in a settings field must pause before the settings are saved.
const SAVE_DELAY: Duration = Duration::from_millis(500);

pub struct State {
    pub app: AppHandle<State>,
    pub(super) rt: Handle,
    pub(super) http: reqwest::Client,
    pub settings: Settings,
    pub(super) settings_path: PathBuf,
    pub page: Page,
    /// The text to work on: the selection, editable.
    pub source: TextEdit,
    pub answer: Option<Answer>,
    /// Show the fixed text as a word diff rather than plain text.
    pub show_changes: bool,
    /// Keep the window open when another app takes focus.
    pub pinned: bool,
    /// The language macOS prefers, of those the window speaks.
    system_locale: Locale,
    /// The dropdown that's open.
    pub menu: Option<Menu>,
    /// Whether the open dropdown's list shows yet: it waits for the window to
    /// grow, since a window that fits its content doesn't lay out again when
    /// its height changes, and the list would be cut to the old height.
    pub menu_shown: bool,
    /// A one-line note under the text: nothing selected, copied, a save error.
    pub notice: Option<String>,
    /// Whether Fettle may copy and paste in other apps.
    pub trusted: bool,
    pub form: Form,
    pub update: Update,
    pub(super) shortcut: Option<ShortcutId>,
    /// Whether the selection is being copied, so a second press can't copy
    /// over the first one's borrowed pasteboard.
    capturing: bool,
    /// A save waiting for typing to pause.
    pending_save: Option<JoinHandle<()>>,
    /// Counts runs, so a late piece of an old answer is dropped.
    run: u64,
    /// The window's top-left in physical pixels, once Fettle has placed it.
    position: Option<(i32, i32)>,
    /// Where a drag of the window began.
    drag: Option<Drag>,
}

/// The pointer and the window's top-left when a drag of the window began.
#[derive(Clone, Copy)]
struct Drag {
    pointer: (f64, f64),
    origin: (i32, i32),
}

/// How Fettle starts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Show the settings.
    Shown,
    /// Stay hidden until the shortcut (how it runs at login).
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Language,
    Model,
    AppLanguage,
    Rewrite,
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
    Done {
        text: String,
        /// For Fix: the word diff from the original to `text`.
        changes: Vec<Piece>,
        copied: bool,
    },
    Failed(String),
}

impl State {
    pub fn new(
        app: AppHandle<State>,
        rt: Handle,
        http: reqwest::Client,
        settings: Settings,
        settings_path: PathBuf,
    ) -> Self {
        let form = Form::new(&settings);
        Self {
            app,
            rt,
            http,
            settings,
            settings_path,
            page: Page::Main,
            source: TextEdit::new(""),
            answer: None,
            show_changes: true,
            pinned: false,
            system_locale: Locale::from_preferred(&mac::preferred_languages()),
            menu: None,
            menu_shown: false,
            notice: None,
            trusted: mac::accessibility_trusted(),
            form,
            update: Update::Idle,
            shortcut: None,
            capturing: false,
            pending_save: None,
            run: 0,
            position: None,
            drag: None,
        }
    }

    /// Take the shortcut, and show the settings if `start` says so.
    pub fn started(&mut self, start: Start) {
        self.apply_appearance();
        self.register_saved_shortcut();
        eprintln!(
            "fettle {} is running; {} opens it",
            crate::VERSION,
            self.settings.shortcut
        );
        if start == Start::Shown {
            self.open_settings();
        }
    }

    pub(super) fn apply_appearance(&self) {
        mac::set_appearance(self.settings.appearance);
    }

    /// The shortcut: hide the window if it shows, else copy the selection
    /// and show it.
    pub fn summon(&mut self) {
        if self.capturing {
            return;
        }
        if self.app.is_visible() {
            self.hide();
            return;
        }
        self.set_trusted(mac::accessibility_trusted());
        if !self.trusted {
            self.open_settings();
            self.form.error = Some(FormError::Accessibility);
            return;
        }
        self.capturing = true;
        let app = self.app.clone();
        // Copying waits for the other app, off the UI thread.
        self.rt.spawn_blocking(move || {
            let text = mac::copy_selection();
            app.update(move |s, _| s.show_text(text));
        });
    }

    fn show_text(&mut self, text: Option<String>) {
        self.capturing = false;
        self.cancel();
        self.page = Page::Main;
        self.answer = None;
        self.menu = None;
        self.notice = match &text {
            Some(_) => None,
            None => Some(self.t().no_selection.into()),
        };
        self.source.replace_text(text.unwrap_or_default());
        self.show();
    }

    pub(super) fn show(&mut self) {
        // Not `AppHandle::place`: it pins the window's size, and the window fits its content.
        if !self.app.is_visible()
            && let Some(position) = mac::top_center_under_pointer(WIDTH, TOP_MARGIN)
        {
            self.position = Some(position);
            self.app.set_position(position);
        }
        self.app.open();
    }

    pub fn hide(&mut self) {
        self.flush_save();
        self.cancel();
        self.menu = None;
        self.app.hide();
    }

    /// The window lost focus: hide it, unless it's pinned.
    pub fn blurred(&mut self) {
        if !self.pinned && !self.form.shortcut.is_recording() {
            self.hide();
        }
    }

    /// Return the window's text in the language it speaks.
    pub fn t(&self) -> &'static Strings {
        self.locale().strings()
    }

    /// Return the language the window speaks: the chosen one, or macOS's.
    pub fn locale(&self) -> Locale {
        self.settings.app_language.unwrap_or(self.system_locale)
    }

    pub fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
    }

    pub fn clear_source(&mut self) {
        self.source.replace_text(String::new());
    }

    /// Move the window with the pointer. The panel is outside winit's reach, so
    /// `.window_drag()` can't move it.
    pub fn drag(&mut self, phase: Phase) {
        match phase {
            Phase::Down => {
                self.drag = mac::pointer()
                    .zip(self.position)
                    .map(|(pointer, origin)| Drag { pointer, origin });
            }
            Phase::Move => {
                let (Some(Drag { pointer, origin }), Some(now)) = (self.drag, mac::pointer())
                else {
                    return;
                };
                let at = (
                    origin.0 + (now.0 - pointer.0).round() as i32,
                    origin.1 + (now.1 - pointer.1).round() as i32,
                );
                self.position = Some(at);
                self.app.set_position(at);
            }
            Phase::Up => self.drag = None,
        }
    }

    pub fn toggle_menu(&mut self, menu: Menu) {
        if self.menu == Some(menu) {
            self.menu = None;
            return;
        }
        self.menu = Some(menu);
        self.menu_shown = false;
        let app = self.app.clone();
        self.rt.spawn(async move {
            tokio::time::sleep(MENU_DELAY).await;
            app.update(move |s, _| s.menu_shown = s.menu == Some(menu));
        });
    }

    /// Whether `menu`'s list shows.
    pub fn menu_open(&self, menu: Menu) -> bool {
        self.menu == Some(menu) && self.menu_shown
    }

    /// Run `action` on the text in the window.
    pub fn start(&mut self, action: Action) {
        self.menu = None;
        let original = self.source.text.clone();
        if original.trim().is_empty() {
            self.notice = Some(self.t().no_text.into());
            return;
        }
        self.notice = None;
        self.cancel();
        let backend = match self.settings.missing() {
            Some(missing) => Err(missing),
            None => Backend::of(&self.settings),
        };
        let backend = match backend {
            Ok(backend) => backend,
            Err(missing) => {
                self.answer = Some(Answer {
                    action,
                    status: Status::Failed(self.t().missing(missing).into()),
                });
                return;
            }
        };
        let run = self.run;
        let ask = Ask {
            system: prompts::system(action, &self.settings.language),
            text: original,
            model: self.settings.model().trim().to_owned(),
            effort: self.settings.reasoning.effort(),
        };
        let (app, http) = (self.app.clone(), self.http.clone());
        let empty = self.t().empty_answer;
        let job = self.rt.spawn(async move {
            let mut streamed = String::new();
            let result = async {
                let backend = fresh_backend(&app, &http, backend).await?;
                backend
                    .stream(&http, &ask, |delta| {
                        streamed.push_str(delta);
                        let delta = delta.to_owned();
                        app.update(move |s, _| s.delta(run, &delta));
                    })
                    .await
            }
            .await;
            let status = match result {
                // The diff can take a while on a long text: keep it off the UI thread.
                Ok(()) => tokio::task::spawn_blocking(move || {
                    complete(action, &ask.text, &streamed, empty)
                })
                .await
                .unwrap_or_else(|e| Status::Failed(e.to_string())),
                Err(e) => Status::Failed(format!("{e:#}")),
            };
            app.update(move |s, _| s.finished(run, status));
        });
        self.answer = Some(Answer {
            action,
            status: Status::Running {
                run,
                job,
                text: String::new(),
            },
        });
    }

    /// The text run `run` is streaming into, if it's still the one running.
    fn running_text(&mut self, run: u64) -> Option<&mut String> {
        match &mut self.answer.as_mut()?.status {
            Status::Running {
                run: current, text, ..
            } if *current == run => Some(text),
            _ => None,
        }
    }

    fn delta(&mut self, run: u64, delta: &str) {
        if let Some(text) = self.running_text(run) {
            text.push_str(delta);
        }
    }

    fn finished(&mut self, run: u64, status: Status) {
        if self.running_text(run).is_none() {
            return;
        }
        if let Some(answer) = self.answer.as_mut() {
            answer.status = status;
        }
    }

    fn cancel(&mut self) {
        self.run += 1;
        let stopped = self.t().stopped;
        if let Some(answer) = self.answer.as_mut()
            && let Status::Running { job, .. } = &answer.status
        {
            job.abort();
            answer.status = Status::Failed(stopped.into());
        }
    }

    pub fn stop(&mut self) {
        self.cancel();
    }

    /// The finished answer's text.
    pub fn result(&self) -> Option<&str> {
        match &self.answer.as_ref()?.status {
            Status::Done { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Hide the window and paste the answer over the selection.
    pub fn replace(&mut self) {
        let Some(text) = self.result().map(str::to_owned) else {
            return;
        };
        self.hide();
        self.rt.spawn_blocking(move || {
            std::thread::sleep(PASTE_DELAY);
            mac::paste(&text);
        });
    }

    pub fn copy(&mut self) {
        let Some(text) = self.result().map(str::to_owned) else {
            return;
        };
        mac::copy(&text);
        if let Some(Answer {
            status: Status::Done { copied, .. },
            ..
        }) = self.answer.as_mut()
        {
            *copied = true;
        }
    }

    pub fn set_language(&mut self, language: &str) {
        self.menu = None;
        self.settings.language = language.to_owned();
        self.save();
        if self
            .answer
            .as_ref()
            .is_some_and(|a| a.action == Action::Translate)
        {
            self.start(Action::Translate);
        }
    }

    pub fn toggle_favourite(&mut self, language: &str) {
        self.settings.toggle_favourite(language);
        self.save();
    }

    pub(super) fn save(&mut self) {
        if let Some(job) = self.pending_save.take() {
            job.abort();
        }
        if let Err(e) = self.settings.save(&self.settings_path) {
            self.report(fill(self.t().save_failed, format_args!("{e:#}")));
        }
    }

    /// Save the settings once typing pauses, rather than on every keystroke.
    pub(super) fn save_soon(&mut self) {
        if let Some(job) = self.pending_save.take() {
            job.abort();
        }
        let app = self.app.clone();
        self.pending_save = Some(self.rt.spawn(async move {
            tokio::time::sleep(SAVE_DELAY).await;
            app.update(|s, _| {
                s.pending_save = None;
                s.save();
            });
        }));
    }

    /// Save now if a save is waiting.
    pub(super) fn flush_save(&mut self) {
        if self.pending_save.is_some() {
            self.save();
        }
    }

    /// Quit, saving what's unsaved first.
    pub fn quit(&mut self, tx: &mut Tx) {
        self.flush_save();
        tx.quit();
    }

    /// Show `message` on the page that's open.
    pub(super) fn report(&mut self, message: String) {
        match self.page {
            Page::Main => self.notice = Some(message),
            Page::Settings => self.form.error = Some(FormError::Other(message)),
        }
    }
}

/// The status of an `action` on `original` whose answer streamed in full as
/// `text`: done, with Fix's word diff, unless the model returned nothing, which
/// fails with `empty`.
fn complete(action: Action, original: &str, text: &str, empty: &str) -> Status {
    if text.trim().is_empty() {
        return Status::Failed(empty.into());
    }
    // Models often add or drop a final newline; keep the selection's.
    let end = &original[original.trim_end().len()..];
    let text = format!("{}{end}", text.trim_end());
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

/// `backend` with a fresh ChatGPT token, saving a refreshed one.
pub(super) async fn fresh_backend(
    app: &AppHandle<State>,
    http: &reqwest::Client,
    backend: Backend,
) -> anyhow::Result<Backend> {
    let Backend::ChatGpt(auth) = &backend else {
        return Ok(backend);
    };
    match chatgpt::fresh(http, auth).await? {
        Some(auth) => {
            let saved = auth.clone();
            app.update(move |s, _| s.saved_auth(saved));
            Ok(Backend::ChatGpt(auth))
        }
        None => Ok(backend),
    }
}
