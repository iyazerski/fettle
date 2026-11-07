//! The window's state and what changes it on the main page: showing the
//! selection, running an action, and moving the window. The settings page's
//! part is in [`settings_state`](super::settings_state).

use std::path::PathBuf;
use std::time::Duration;

use rune::{AppHandle, Phase, ShortcutId, TextEdit};
use rune_kit::ShortcutRecorderState;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use super::view::WIDTH;
use crate::diff::{self, Piece};
use crate::llm::{Ask, Backend, chatgpt};
use crate::prompts::{self, Action};
use crate::settings::{Appearance, Settings};
use crate::{login_item, mac};

/// How far below the top of the screen the window opens.
const TOP_MARGIN: f32 = 120.0;
/// How long the window waits, hidden, before pasting, so the app the text
/// came from has its key window back.
const PASTE_DELAY: Duration = Duration::from_millis(120);
/// How long the window takes to grow for a dropdown.
const MENU_DELAY: Duration = Duration::from_millis(200);

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
    /// Show the proofread text as a word diff rather than plain text.
    pub show_changes: bool,
    /// The dropdown that's open.
    pub menu: Option<Menu>,
    /// Whether the open dropdown's list shows yet: it waits for the window to
    /// grow, since a window that fits its content doesn't lay out again when
    /// its height changes, and the list would be cut to the old height.
    pub menu_shown: bool,
    /// A one-line note under the text: nothing selected, copied, a save error.
    pub notice: Option<String>,
    /// Whether Quill may copy and paste in other apps.
    pub trusted: bool,
    pub form: Form,
    pub update: Update,
    pub(super) shortcut: Option<ShortcutId>,
    job: Option<JoinHandle<()>>,
    /// Counts runs, so a late piece of an old answer is dropped.
    run: u64,
    /// The window's top-left in physical pixels, once Quill has placed it.
    position: Option<(i32, i32)>,
    /// The pointer and the window's position when a drag of the window began.
    drag: Option<((f64, f64), (i32, i32))>,
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
}

pub struct Answer {
    pub action: Action,
    /// The text the action ran on.
    pub original: String,
    pub text: String,
    pub status: Status,
    /// For Proofread, once done: the word diff from `original` to `text`.
    pub changes: Vec<Piece>,
    pub copied: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    Done,
    Failed(String),
}

/// The settings page's own state.
pub struct Form {
    pub api_base: TextEdit,
    pub api_key: TextEdit,
    pub shortcut: ShortcutRecorderState,
    pub models: Models,
    pub login: Login,
    pub at_login: bool,
    /// A problem to show at the top of the page.
    pub error: Option<String>,
}

pub enum Models {
    Idle,
    Loading,
    Ready(Vec<String>),
    Failed(String),
}

pub enum Login {
    Idle,
    Waiting(JoinHandle<()>),
    Failed(String),
}

/// Where checking for a new version stands.
pub enum Update {
    Idle,
    Checking,
    Latest,
    Available(String),
    Installing(String),
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
        let form = Form {
            api_base: TextEdit::new(&settings.api_base),
            api_key: TextEdit::new(&settings.api_key).secure(),
            shortcut: ShortcutRecorderState::new(settings.shortcut.parse().ok()),
            models: Models::Idle,
            login: Login::Idle,
            at_login: login_item::enabled(),
            error: None,
        };
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
            menu: None,
            menu_shown: false,
            notice: None,
            trusted: mac::accessibility_trusted(false),
            form,
            update: Update::Idle,
            shortcut: None,
            job: None,
            run: 0,
            position: None,
            drag: None,
        }
    }

    /// Take the shortcut, and unless started in the background, show the settings.
    pub fn started(&mut self, background: bool) {
        self.apply_appearance();
        self.register_saved_shortcut();
        eprintln!(
            "quill {} is running; {} opens it",
            crate::VERSION,
            self.settings.shortcut
        );
        if !background {
            self.open_settings();
        }
    }

    /// Whether the window draws dark.
    pub fn dark(&self, system_dark: bool) -> bool {
        match self.settings.appearance {
            Appearance::System => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }

    pub(super) fn apply_appearance(&self) {
        mac::set_dark(match self.settings.appearance {
            Appearance::System => None,
            Appearance::Light => Some(false),
            Appearance::Dark => Some(true),
        });
    }

    /// The shortcut: hide the window if it shows, else copy the selection
    /// and show it.
    pub fn summon(&mut self) {
        if self.app.is_visible() {
            self.hide();
            return;
        }
        self.trusted = mac::accessibility_trusted(false);
        if !self.trusted {
            self.open_settings();
            self.form.error =
                Some("Allow Quill in Accessibility so it can copy the selected text.".into());
            return;
        }
        let app = self.app.clone();
        // Copying waits for the other app, off the UI thread.
        std::thread::spawn(move || {
            let text = mac::copy_selection();
            app.update(move |s, _| s.show_text(text));
        });
    }

    fn show_text(&mut self, text: Option<String>) {
        self.cancel();
        self.page = Page::Main;
        self.answer = None;
        self.menu = None;
        self.notice = match &text {
            Some(_) => None,
            None => Some("No text selected. Type or paste it here.".into()),
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
        self.cancel();
        self.menu = None;
        self.app.hide();
    }

    /// The window lost focus: hide it, unless the browser took focus for the sign-in.
    pub fn blurred(&mut self) {
        if !matches!(self.form.login, Login::Waiting(_)) && !self.form.shortcut.is_recording() {
            self.hide();
        }
    }

    /// Move the window with the pointer. The panel is outside winit's reach, so
    /// `.window_drag()` can't move it.
    pub fn drag(&mut self, phase: Phase) {
        match phase {
            Phase::Down => self.drag = mac::pointer().zip(self.position),
            Phase::Move => {
                let (Some((start, origin)), Some(now)) = (self.drag, mac::pointer()) else {
                    return;
                };
                let at = (
                    origin.0 + (now.0 - start.0).round() as i32,
                    origin.1 + (now.1 - start.1).round() as i32,
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
        std::thread::spawn(move || {
            std::thread::sleep(MENU_DELAY);
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
            self.notice = Some("There's no text to work on.".into());
            return;
        }
        self.notice = None;
        self.cancel();
        self.run += 1;
        let run = self.run;
        let mut answer = Answer {
            action,
            original: original.clone(),
            text: String::new(),
            status: Status::Running,
            changes: Vec::new(),
            copied: false,
        };
        let backend = match (self.settings.missing(), Backend::of(&self.settings)) {
            (None, Some(backend)) => backend,
            (missing, _) => {
                answer.status =
                    Status::Failed(missing.unwrap_or("Set up a model in Settings").into());
                self.answer = Some(answer);
                return;
            }
        };
        self.answer = Some(answer);
        let ask = Ask {
            system: prompts::system(action, &self.settings.language),
            text: original,
            model: self.settings.model().trim().to_owned(),
            effort: self.settings.reasoning.effort(),
        };
        let (app, http) = (self.app.clone(), self.http.clone());
        self.job = Some(self.rt.spawn(async move {
            let result = async {
                let backend = fresh_backend(&app, &http, backend).await?;
                let to_ui = app.clone();
                backend
                    .stream(&http, &ask, move |delta| {
                        let delta = delta.to_owned();
                        to_ui.update(move |s, _| s.delta(run, &delta));
                    })
                    .await
            }
            .await
            .map_err(|e| format!("{e:#}"));
            app.update(move |s, _| s.finished(run, result));
        }));
    }

    fn delta(&mut self, run: u64, delta: &str) {
        if let Some(answer) = self.answer.as_mut().filter(|_| run == self.run) {
            answer.text.push_str(delta);
        }
    }

    fn finished(&mut self, run: u64, result: Result<(), String>) {
        if run != self.run {
            return;
        }
        self.job = None;
        let Some(answer) = self.answer.as_mut() else {
            return;
        };
        match result {
            Ok(()) if answer.text.trim().is_empty() => {
                answer.status = Status::Failed("The model returned nothing.".into())
            }
            Ok(()) => {
                // Models often add or drop a final newline; keep the selection's.
                let kept = answer.original.len() - answer.original.trim_end().len();
                let end = &answer.original[answer.original.len() - kept..];
                answer.text = format!("{}{end}", answer.text.trim_end());
                if answer.action == Action::Proofread {
                    answer.changes = diff::words(&answer.original, &answer.text);
                }
                answer.status = Status::Done;
            }
            Err(e) => answer.status = Status::Failed(e),
        }
    }

    fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.abort();
        }
        self.run += 1;
        if let Some(answer) = self.answer.as_mut().filter(|a| a.status == Status::Running) {
            answer.status = Status::Failed("Stopped.".into());
        }
    }

    pub fn stop(&mut self) {
        self.cancel();
    }

    /// The finished answer's text.
    pub fn result(&self) -> Option<&str> {
        self.answer
            .as_ref()
            .filter(|a| a.status == Status::Done)
            .map(|a| a.text.as_str())
    }

    /// Hide the window and paste the answer over the selection.
    pub fn replace(&mut self) {
        let Some(text) = self.result().map(str::to_owned) else {
            return;
        };
        self.hide();
        std::thread::spawn(move || {
            std::thread::sleep(PASTE_DELAY);
            mac::paste(&text);
        });
    }

    pub fn copy(&mut self) {
        let Some(text) = self.result().map(str::to_owned) else {
            return;
        };
        mac::copy(&text);
        if let Some(answer) = self.answer.as_mut() {
            answer.copied = true;
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
        if let Err(e) = self.settings.save(&self.settings_path) {
            self.report(format!("Couldn't save the settings: {e:#}"));
        }
    }

    /// Show `message` on the page that's open.
    pub(super) fn report(&mut self, message: String) {
        match self.page {
            Page::Main => self.notice = Some(message),
            Page::Settings => self.form.error = Some(message),
        }
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
