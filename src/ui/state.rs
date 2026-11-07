//! The window's state and what changes it: showing the selection, running an
//! action, and the settings.

use std::path::PathBuf;
use std::time::Duration;

use rune::{AppHandle, Shortcut, ShortcutError, ShortcutId, TextEdit};
use rune_kit::ShortcutRecorderState;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use super::view::WIDTH;
use crate::diff::{self, Piece};
use crate::llm::{Ask, Backend, chatgpt};
use crate::prompts::{self, Action};
use crate::settings::{Provider, Reasoning, Settings};
use crate::{login_item, mac};

/// How far below the top of the screen the window opens.
const TOP_MARGIN: f32 = 120.0;
/// How long the window waits, hidden, before pasting, so the app the text
/// came from has its key window back.
const PASTE_DELAY: Duration = Duration::from_millis(120);

pub struct State {
    pub app: AppHandle<State>,
    rt: Handle,
    http: reqwest::Client,
    pub settings: Settings,
    settings_path: PathBuf,
    pub page: Page,
    /// The text to work on: the selection, editable.
    pub source: TextEdit,
    pub answer: Option<Answer>,
    /// Show the improvement as a word diff rather than plain text.
    pub show_changes: bool,
    pub language_open: bool,
    /// A one-line note under the text: nothing selected, copied, a save error.
    pub notice: Option<String>,
    /// Whether Quill may copy and paste in other apps.
    pub trusted: bool,
    pub form: Form,
    shortcut: Option<ShortcutId>,
    job: Option<JoinHandle<()>>,
    /// Counts runs, so a late piece of an old answer is dropped.
    run: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Settings,
}

pub struct Answer {
    pub action: Action,
    /// The text the action ran on.
    pub original: String,
    pub text: String,
    pub status: Status,
    /// For Improve, once done: the word diff from `original` to `text`.
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
    pub model: TextEdit,
    pub shortcut: ShortcutRecorderState,
    pub models: Models,
    pub login: Login,
    pub at_login: bool,
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
            model: TextEdit::new(settings.model()),
            shortcut: ShortcutRecorderState::new(settings.shortcut.parse().ok()),
            models: Models::Idle,
            login: Login::Idle,
            at_login: login_item::enabled(),
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
            language_open: false,
            notice: None,
            trusted: mac::accessibility_trusted(false),
            form,
            shortcut: None,
            job: None,
            run: 0,
        }
    }

    /// Take the shortcut, and unless started in the background, show the settings.
    pub fn started(&mut self, background: bool) {
        mac::use_light_appearance();
        match self.settings.shortcut.parse::<Shortcut>() {
            Ok(shortcut) => {
                if let Err(e) = self.register(shortcut.clone()) {
                    self.form.shortcut.mark_unregistered();
                    self.notice = Some(self.shortcut_message(&e, &shortcut));
                }
            }
            Err(e) => self.notice = Some(format!("The saved shortcut is broken: {e}")),
        }
        eprintln!(
            "quill {} is running; {} opens it",
            crate::VERSION,
            self.settings.shortcut
        );
        if !background {
            self.open_settings();
        }
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
            self.notice =
                Some("Allow Quill in Accessibility so it can copy the selected text.".into());
            self.open_settings();
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
        self.language_open = false;
        self.notice = match &text {
            Some(_) => None,
            None => Some("No text selected. Type or paste it here.".into()),
        };
        self.source.replace_text(text.unwrap_or_default());
        self.show();
    }

    pub fn open_settings(&mut self) {
        self.trusted = mac::accessibility_trusted(false);
        self.form.at_login = login_item::enabled();
        self.page = Page::Settings;
        self.show();
    }

    pub fn back(&mut self) {
        self.page = Page::Main;
        self.notice = None;
    }

    fn show(&mut self) {
        // Not `AppHandle::place`: it pins the window's size, and the window fits its content.
        if !self.app.is_visible()
            && let Some(position) = mac::top_center_under_pointer(WIDTH, TOP_MARGIN)
        {
            self.app.set_position(position);
        }
        self.app.open();
    }

    pub fn hide(&mut self) {
        self.cancel();
        self.language_open = false;
        self.app.hide();
    }

    /// The window lost focus: hide it, unless the browser took focus for the sign-in.
    pub fn blurred(&mut self) {
        if !matches!(self.form.login, Login::Waiting(_)) && !self.form.shortcut.is_recording() {
            self.hide();
        }
    }

    /// Run `action` on the text in the window.
    pub fn start(&mut self, action: Action) {
        self.language_open = false;
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
                if answer.action == Action::Improve {
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
        self.language_open = false;
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

    // Settings.

    fn save(&mut self) {
        if let Err(e) = self.settings.save(&self.settings_path) {
            self.notice = Some(format!("Couldn't save the settings: {e:#}"));
        }
    }

    pub fn set_provider(&mut self, provider: Provider) {
        if self.settings.provider == provider {
            return;
        }
        self.settings.provider = provider;
        self.form.model.replace_text(self.settings.model());
        self.form.models = Models::Idle;
        self.save();
    }

    pub fn set_reasoning(&mut self, reasoning: Reasoning) {
        self.settings.reasoning = reasoning;
        self.save();
    }

    /// Copy the API fields and the model from the form into the settings.
    pub fn form_edited(&mut self) {
        self.settings.api_base = self.form.api_base.text.trim().to_owned();
        self.settings.api_key = self.form.api_key.text.trim().to_owned();
        *self.settings.model_mut() = self.form.model.text.trim().to_owned();
        self.save();
    }

    pub fn pick_model(&mut self, model: &str) {
        self.form.model.replace_text(model);
        self.form_edited();
    }

    pub fn load_models(&mut self) {
        let Some(backend) = Backend::of(&self.settings) else {
            self.form.models = Models::Failed("Sign in first.".into());
            return;
        };
        self.form.models = Models::Loading;
        let (app, http) = (self.app.clone(), self.http.clone());
        self.rt.spawn(async move {
            let result = async {
                let backend = fresh_backend(&app, &http, backend).await?;
                backend.models(&http).await
            }
            .await;
            app.update(move |s, _| {
                s.form.models = match result {
                    Ok(models) if models.is_empty() => Models::Failed("No models found.".into()),
                    Ok(models) => Models::Ready(models),
                    Err(e) => Models::Failed(format!("{e:#}")),
                };
            });
        });
    }

    pub fn sign_in(&mut self) {
        if let Login::Waiting(job) = &self.form.login {
            job.abort();
        }
        let (app, http) = (self.app.clone(), self.http.clone());
        let job = self.rt.spawn(async move {
            let result = chatgpt::login(&http, |url| {
                if let Err(e) = std::process::Command::new("/usr/bin/open")
                    .arg(url)
                    .status()
                {
                    eprintln!("opening the browser: {e}");
                }
            })
            .await;
            app.update(move |s, _| s.signed_in(result.map_err(|e| format!("{e:#}"))));
        });
        self.form.login = Login::Waiting(job);
    }

    fn signed_in(&mut self, result: Result<crate::settings::ChatGptAuth, String>) {
        match result {
            Ok(auth) => {
                self.form.login = Login::Idle;
                self.settings.chatgpt = Some(auth);
                self.save();
                self.load_models();
                self.open_settings();
            }
            Err(e) => self.form.login = Login::Failed(e),
        }
    }

    pub fn cancel_sign_in(&mut self) {
        if let Login::Waiting(job) = &self.form.login {
            job.abort();
        }
        self.form.login = Login::Idle;
    }

    pub fn sign_out(&mut self) {
        self.settings.chatgpt = None;
        self.form.models = Models::Idle;
        self.save();
    }

    fn saved_auth(&mut self, auth: crate::settings::ChatGptAuth) {
        self.settings.chatgpt = Some(auth);
        self.save();
    }

    pub fn toggle_at_login(&mut self) {
        let result = if self.form.at_login {
            login_item::disable()
        } else {
            login_item::enable()
        };
        match result {
            Ok(()) => self.form.at_login = !self.form.at_login,
            Err(e) => self.notice = Some(format!("{e:#}")),
        }
    }

    pub fn ask_accessibility(&mut self) {
        self.trusted = mac::accessibility_trusted(true);
        if !self.trusted {
            mac::open_accessibility_settings();
        }
    }

    pub fn recheck_accessibility(&mut self) {
        self.trusted = mac::accessibility_trusted(false);
        if self.trusted
            && self
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("Accessibility"))
        {
            self.notice = None;
        }
    }

    /// Move the global shortcut to `shortcut`: take the new one before
    /// letting go of the old, so a refused chord keeps the old working.
    pub fn change_shortcut(&mut self, shortcut: Option<&Shortcut>) -> Result<(), ShortcutError> {
        let Some(shortcut) = shortcut else {
            // Without a shortcut Quill can't be reached at all.
            return Err(ShortcutError::InvalidAccelerator("empty".into()));
        };
        let old = self.shortcut.take();
        if let Err(e) = self.register(shortcut.clone()) {
            self.shortcut = old;
            return Err(e);
        }
        if let Some(old) = old {
            let _ = self.app.unregister_shortcut(old);
        }
        self.settings.shortcut = shortcut.to_string();
        self.save();
        if self
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("shortcut"))
        {
            self.notice = None;
        }
        Ok(())
    }

    fn register(&mut self, shortcut: Shortcut) -> Result<(), ShortcutError> {
        let id = self.app.register_shortcut(shortcut, |s, _| s.summon())?;
        self.shortcut = Some(id);
        Ok(())
    }

    pub fn shortcut_message(&self, error: &ShortcutError, shortcut: &Shortcut) -> String {
        match (error, self.app.shortcut_owner(shortcut)) {
            (ShortcutError::Conflict, Some(owner)) => {
                format!("{owner} already uses the shortcut. Pick another in Settings.")
            }
            _ => format!("Couldn't use the shortcut: {error}. Pick another in Settings."),
        }
    }
}

/// `backend` with a fresh ChatGPT token, saving a refreshed one.
async fn fresh_backend(
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
