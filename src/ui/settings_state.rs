//! The settings page's own state, and what the page changes: where the model
//! runs, which model, the look, the shortcut, the login item, the permission,
//! and updates.

use rune::{Shortcut, ShortcutError, TextEdit, Tx};
use rune_kit::ShortcutRecorderState;

use super::pending::{Pending, Ticket};
use super::state::{Menu, Page, State};
use crate::i18n::{Locale, Strings, fill};
use crate::llm::{Backend, chatgpt, on_device};
use crate::settings::{Appearance, ChatGptAuth, Provider, Settings};
use crate::{login_item, mac, update};

/// System Settings' Apple Intelligence & Siri pane.
const APPLE_INTELLIGENCE_URL: &str = "x-apple.systempreferences:com.apple.Siri-Settings.extension";

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

impl FormError {
    /// Return the problem as the page shows it, in the language of `t`.
    pub fn message<'a>(&'a self, t: &'static Strings) -> &'a str {
        match self {
            FormError::Accessibility => t.accessibility_needed,
            FormError::Shortcut(message) | FormError::Other(message) => message,
        }
    }
}

/// The model list; replacing a load stops it, so a list for an old backend
/// can't arrive late.
pub enum Models {
    Idle,
    Loading(Pending),
    Ready(Vec<String>),
    Failed(String),
}

/// The ChatGPT sign-in in the browser; replacing a wait stops it.
pub enum Login {
    Idle,
    Waiting(Pending),
    Failed(String),
}

/// Where checking for a new version stands. Each step leads only to the
/// next: a check from `Idle`, `Latest` or `Failed`, an install from `Available`.
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
            api_key: TextEdit::new(&settings.credentials.api_key).secure(),
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
        self.recheck_on_device();
        self.form.at_login = login_item::enabled();
        // A release may have come out since the last check, so offer to check again.
        if matches!(self.update, Update::Latest) {
            self.update = Update::Idle;
        }
        self.menu = None;
        self.page = Page::Settings;
        if matches!(self.form.models, Models::Idle) {
            self.load_models();
        }
        self.show();
    }

    pub fn back(&mut self) {
        self.page = Page::Main;
        self.menu = None;
        self.form.error = None;
    }

    pub fn set_provider(&mut self, provider: Provider) {
        if self.settings.provider == provider {
            return;
        }
        self.settings.provider = provider;
        self.form.models = Models::Idle;
        self.save();
        self.load_models();
    }

    pub fn toggle_reasoning(&mut self) {
        self.settings.reasoning_enabled = !self.settings.reasoning_enabled;
        self.save();
    }

    pub fn set_appearance(&mut self, appearance: Appearance) {
        self.settings.appearance = appearance;
        self.apply_appearance();
        self.save();
    }

    /// Show the window in the language the picker lists as `name`, or follow
    /// macOS for any other choice.
    pub fn pick_app_language(&mut self, name: &str) {
        self.menu = None;
        self.settings.app_language = Locale::ALL.into_iter().find(|l| l.native_name() == name);
        self.apply_labels();
        self.save();
    }

    /// Copy the API fields from the form into the settings.
    pub fn api_edited(&mut self) {
        self.settings.api_base = self.form.api_base.text.trim().to_owned();
        self.settings.credentials.api_key = self.form.api_key.text.trim().to_owned();
        self.form.models = Models::Idle;
        self.save();
    }

    /// Open or close the model list, loading it first if needed.
    pub fn toggle_models(&mut self) {
        let opening = !self.menu_is(Menu::Model);
        self.toggle_menu(Menu::Model);
        if opening && !matches!(self.form.models, Models::Ready(_) | Models::Loading(_)) {
            self.load_models();
        }
    }

    pub fn pick_model(&mut self, model: &str) {
        self.menu = None;
        if let Some(current) = self.settings.model_mut() {
            *current = model.to_owned();
            self.save();
        }
    }

    /// Load the provider's models, if it has models to pick and can list them.
    pub fn load_models(&mut self) {
        if self.settings.model().is_none() {
            return;
        }
        let Ok(backend) = Backend::of(&self.settings) else {
            return;
        };
        let client = self.client.clone();
        self.form.models = Models::Loading(Pending::spawn(&self.client, |ticket| async move {
            let result = client.models(backend).await;
            client
                .app()
                .update(move |s, _| s.models_loaded(ticket, result));
        }));
    }

    /// Show the model list load `ticket` returned, unless a newer load or
    /// list replaced it.
    fn models_loaded(&mut self, ticket: Ticket, result: anyhow::Result<Vec<String>>) {
        if !matches!(&self.form.models, Models::Loading(load) if load.answers(ticket)) {
            return;
        }
        self.form.models = match result {
            Ok(models) if models.is_empty() => Models::Failed(self.t().no_models.into()),
            Ok(models) => Models::Ready(models),
            Err(e) => Models::Failed(fill(self.t().list_models_failed, format_args!("{e:#}"))),
        };
        if !matches!(self.form.models, Models::Ready(_)) && self.menu_is(Menu::Model) {
            self.menu = None;
        }
    }

    pub fn sign_in(&mut self) {
        let client = self.client.clone();
        let waiting = Pending::spawn(&self.client, |ticket| async move {
            let result = chatgpt::login(client.http(), |url| {
                if let Err(e) = std::process::Command::new("/usr/bin/open")
                    .arg(url)
                    .status()
                {
                    eprintln!("opening the browser: {e}");
                }
            })
            .await;
            let result = result.map_err(|e| format!("{e:#}"));
            client.app().update(move |s, _| s.signed_in(ticket, result));
        });
        self.form.login = Login::Waiting(waiting);
        // The window floats above every app, so it would cover the browser's sign-in
        // page. It comes back with the result.
        self.hide();
    }

    /// Take the result of sign-in `ticket`, unless it was cancelled or
    /// started again.
    fn signed_in(&mut self, ticket: Ticket, result: Result<ChatGptAuth, String>) {
        if !matches!(&self.form.login, Login::Waiting(waiting) if waiting.answers(ticket)) {
            return;
        }
        match result {
            Ok(auth) => {
                self.form.login = Login::Idle;
                self.settings.credentials.chatgpt = Some(auth);
                self.save();
                self.load_models();
            }
            Err(e) => self.form.login = Login::Failed(e),
        }
        // Back to where the sign-in started: the welcome steps or the settings.
        if matches!(self.page, Page::Onboarding(_)) {
            self.show();
        } else {
            self.open_settings();
        }
    }

    pub fn cancel_sign_in(&mut self) {
        self.form.login = Login::Idle;
    }

    pub fn sign_out(&mut self) {
        self.settings.credentials.chatgpt = None;
        self.form.models = Models::Idle;
        self.menu = None;
        self.save();
    }

    /// Keep `auth`, refreshed from the sign-in with refresh token `old_refresh`,
    /// unless that sign-in has since been signed out or replaced.
    pub(super) fn saved_auth(&mut self, old_refresh: &str, auth: ChatGptAuth) {
        if self
            .settings
            .credentials
            .chatgpt
            .as_ref()
            .is_none_or(|current| current.refresh != old_refresh)
        {
            return;
        }
        self.settings.credentials.chatgpt = Some(auth);
        self.save();
    }

    pub fn toggle_underlines(&mut self) {
        self.settings.underline_mistakes = !self.settings.underline_mistakes;
        self.apply_underlines();
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

    /// Ask macOS for Accessibility: its own prompt the first time, which lists
    /// Fettle in System Settings, then System Settings itself, since macOS
    /// prompts only once per launch.
    pub fn ask_accessibility(&mut self) {
        if self.asked_accessibility {
            self.recheck_accessibility();
            if !self.trusted {
                mac::open_accessibility_settings();
            }
            return;
        }
        self.asked_accessibility = true;
        self.set_trusted(mac::request_accessibility());
    }

    pub fn recheck_accessibility(&mut self) {
        self.set_trusted(mac::accessibility_trusted());
    }

    pub fn recheck_on_device(&mut self) {
        self.on_device = on_device::availability();
    }

    /// Open Apple Intelligence & Siri in System Settings.
    pub fn open_apple_intelligence(&mut self) {
        mac::open_url(APPLE_INTELLIGENCE_URL);
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
                self.form.error = Some(FormError::Shortcut(fill(self.t().shortcut_broken, e)))
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
            (ShortcutError::Conflict, Some(owner)) => fill(self.t().shortcut_taken, owner),
            _ => fill(self.t().shortcut_failed, error),
        }
    }

    /// Look for a new version, unless a check or an install is under way.
    pub fn check_update(&mut self) {
        if matches!(self.update, Update::Checking | Update::Installing(_)) {
            return;
        }
        self.update = Update::Checking;
        let client = self.client.clone();
        self.client.spawn(async move {
            let result = update::available(client.http()).await;
            client.app().update(move |s, _| {
                if !matches!(s.update, Update::Checking) {
                    return;
                }
                s.update = match result {
                    Ok(Some(version)) => Update::Available(version),
                    Ok(None) => Update::Latest,
                    Err(e) => Update::Failed(fill(s.t().check_failed, format_args!("{e:#}"))),
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
        let client = self.client.clone();
        self.client.spawn(async move {
            let result = update::install(client.http(), &version).await;
            client.app().update(move |s, tx: &mut Tx| {
                match result.and_then(|bundle| update::relaunch(&bundle)) {
                    Ok(()) => tx.quit(),
                    Err(e) => {
                        s.update = Update::Failed(fill(s.t().update_failed, format_args!("{e:#}")))
                    }
                }
            });
        });
    }
}
