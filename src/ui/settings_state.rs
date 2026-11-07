//! The settings page's own state, and what the page changes: where the model
//! runs, which model, the look, the shortcut, the login item, the permission,
//! and updates.

use std::fmt;

use rune::{Shortcut, ShortcutError, TextEdit, Tx};
use rune_kit::ShortcutRecorderState;
use tokio::task::JoinHandle;

use super::state::{Menu, Page, State, fresh_backend};
use crate::llm::{Backend, chatgpt};
use crate::settings::{Appearance, ChatGptAuth, Provider, Reasoning, Settings};
use crate::{login_item, mac, update};

/// The settings page's own state.
pub struct Form {
    pub api_base: TextEdit,
    pub api_key: TextEdit,
    pub shortcut: ShortcutRecorderState,
    pub models: Models,
    pub login: Login,
    pub at_login: bool,
    /// A problem to show at the top of the page.
    pub error: Option<FormError>,
}

/// A problem on the settings page, kept apart by what clears it.
pub enum FormError {
    /// Fettle may not copy and paste: cleared once it may.
    Accessibility,
    /// The shortcut didn't register: cleared by one that does.
    Shortcut(String),
    Other(String),
}

impl fmt::Display for FormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormError::Accessibility => {
                f.write_str("Allow Fettle in Accessibility so it can copy the selected text.")
            }
            FormError::Shortcut(message) | FormError::Other(message) => f.write_str(message),
        }
    }
}

pub enum Models {
    Idle,
    Loading(JoinHandle<()>),
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

impl Form {
    pub(super) fn new(settings: &Settings) -> Self {
        Self {
            api_base: TextEdit::new(&settings.api_base),
            api_key: TextEdit::new(&settings.api_key).secure(),
            shortcut: ShortcutRecorderState::new(settings.shortcut.parse().ok()),
            models: Models::Idle,
            login: Login::Idle,
            at_login: login_item::enabled(),
            error: None,
        }
    }
}

impl State {
    pub fn open_settings(&mut self) {
        self.set_trusted(mac::accessibility_trusted());
        self.form.at_login = login_item::enabled();
        self.menu = None;
        self.page = Page::Settings;
        if matches!(self.form.models, Models::Idle) && Backend::of(&self.settings).is_ok() {
            self.load_models();
        }
        self.show();
    }

    pub fn back(&mut self) {
        self.flush_save();
        self.page = Page::Main;
        self.menu = None;
        self.form.error = None;
    }

    pub fn set_provider(&mut self, provider: Provider) {
        if self.settings.provider == provider {
            return;
        }
        self.settings.provider = provider;
        self.set_models(Models::Idle);
        self.save();
        if Backend::of(&self.settings).is_ok() {
            self.load_models();
        }
    }

    pub fn set_reasoning(&mut self, reasoning: Reasoning) {
        self.settings.reasoning = reasoning;
        self.save();
    }

    pub fn set_appearance(&mut self, appearance: Appearance) {
        self.settings.appearance = appearance;
        self.apply_appearance();
        self.save();
    }

    /// Copy the API fields from the form into the settings.
    pub fn api_edited(&mut self) {
        self.settings.api_base = self.form.api_base.text.trim().to_owned();
        self.settings.api_key = self.form.api_key.text.trim().to_owned();
        self.set_models(Models::Idle);
        self.save_soon();
    }

    /// Open or close the model list, loading it first if needed.
    pub fn toggle_models(&mut self) {
        self.toggle_menu(Menu::Model);
        if self.menu == Some(Menu::Model)
            && !matches!(self.form.models, Models::Ready(_) | Models::Loading(_))
        {
            self.load_models();
        }
    }

    pub fn pick_model(&mut self, model: &str) {
        self.menu = None;
        *self.settings.model_mut() = model.to_owned();
        self.save();
    }

    /// Replace the model list, stopping a load the new list supersedes, so
    /// a list for an old backend can't arrive late.
    fn set_models(&mut self, models: Models) {
        if let Models::Loading(job) = &self.form.models {
            job.abort();
        }
        self.form.models = models;
    }

    pub fn load_models(&mut self) {
        let Ok(backend) = Backend::of(&self.settings) else {
            return;
        };
        let (app, http) = (self.app.clone(), self.http.clone());
        let job = self.rt.spawn(async move {
            let result = async {
                let backend = fresh_backend(&app, &http, backend).await?;
                backend.models(&http).await
            }
            .await;
            app.update(move |s, _| {
                s.form.models = match result {
                    Ok(models) if models.is_empty() => Models::Failed("No models found.".into()),
                    Ok(models) => Models::Ready(models),
                    Err(e) => Models::Failed(format!("Couldn't list the models: {e:#}")),
                };
                if !matches!(s.form.models, Models::Ready(_)) && s.menu == Some(Menu::Model) {
                    s.menu = None;
                }
            });
        });
        self.set_models(Models::Loading(job));
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
        // The window floats above every app, so it would cover the browser's sign-in
        // page. It comes back with the result.
        self.hide();
    }

    fn signed_in(&mut self, result: Result<ChatGptAuth, String>) {
        match result {
            Ok(auth) => {
                self.form.login = Login::Idle;
                self.settings.chatgpt = Some(auth);
                self.save();
                self.load_models();
                self.open_settings();
            }
            Err(e) => {
                self.form.login = Login::Failed(e);
                self.open_settings();
            }
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
        self.set_models(Models::Idle);
        self.menu = None;
        self.save();
    }

    pub(super) fn saved_auth(&mut self, auth: ChatGptAuth) {
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
            Err(e) => self.form.error = Some(FormError::Other(format!("{e:#}"))),
        }
    }

    pub fn ask_accessibility(&mut self) {
        let trusted = mac::request_accessibility();
        self.set_trusted(trusted);
        if !trusted {
            mac::open_accessibility_settings();
        }
    }

    pub fn recheck_accessibility(&mut self) {
        self.set_trusted(mac::accessibility_trusted());
    }

    /// Record whether macOS lets Fettle press keys, and drop the request to
    /// allow it once it does.
    pub(super) fn set_trusted(&mut self, trusted: bool) {
        self.trusted = trusted;
        if trusted && matches!(self.form.error, Some(FormError::Accessibility)) {
            self.form.error = None;
        }
    }

    pub(super) fn register_saved_shortcut(&mut self) {
        match self.settings.shortcut.parse::<Shortcut>() {
            Ok(shortcut) => {
                if let Err(e) = self.register(shortcut.clone()) {
                    self.form.shortcut.mark_unregistered();
                    self.form.error =
                        Some(FormError::Shortcut(self.shortcut_message(&e, &shortcut)));
                }
            }
            Err(e) => {
                self.form.error = Some(FormError::Shortcut(format!(
                    "The saved shortcut is broken: {e}"
                )))
            }
        }
    }

    /// Move the global shortcut to `shortcut`: take the new one before
    /// letting go of the old, so a refused chord keeps the old working.
    pub fn change_shortcut(&mut self, shortcut: Option<&Shortcut>) -> Result<(), ShortcutError> {
        let Some(shortcut) = shortcut else {
            // Without a shortcut Fettle can't be reached at all.
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
        if matches!(self.form.error, Some(FormError::Shortcut(_))) {
            self.form.error = None;
        }
        Ok(())
    }

    fn register(&mut self, shortcut: Shortcut) -> Result<(), ShortcutError> {
        let id = self.app.register_shortcut(shortcut, |s, _| s.summon())?;
        self.shortcut = Some(id);
        Ok(())
    }

    fn shortcut_message(&self, error: &ShortcutError, shortcut: &Shortcut) -> String {
        match (error, self.app.shortcut_owner(shortcut)) {
            (ShortcutError::Conflict, Some(owner)) => {
                format!("{owner} already uses the shortcut. Pick another below.")
            }
            _ => format!("Couldn't use the shortcut: {error}. Pick another below."),
        }
    }

    pub fn check_update(&mut self) {
        self.update = Update::Checking;
        let (app, http) = (self.app.clone(), self.http.clone());
        self.rt.spawn(async move {
            let result = update::latest(&http).await;
            app.update(move |s, _| {
                s.update = match result {
                    Ok(version) if update::newer(&version, crate::VERSION) => {
                        Update::Available(version)
                    }
                    Ok(_) => Update::Latest,
                    Err(e) => Update::Failed(format!("Couldn't check for updates: {e:#}")),
                };
            });
        });
    }

    /// Download the new version, put it in place, and restart into it.
    pub fn install_update(&mut self) {
        let Update::Available(version) = &self.update else {
            return;
        };
        let version = version.clone();
        self.update = Update::Installing(version.clone());
        let (app, http) = (self.app.clone(), self.http.clone());
        self.rt.spawn(async move {
            let result = update::install(&http, &version).await;
            app.update(move |s, tx: &mut Tx| {
                match result.and_then(|bundle| update::relaunch(&bundle)) {
                    Ok(()) => tx.quit(),
                    Err(e) => s.update = Update::Failed(format!("Couldn't update: {e:#}")),
                }
            });
        });
    }
}
