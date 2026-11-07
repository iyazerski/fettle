//! What the settings page changes: where the model runs, which model, the
//! look, the shortcut, the login item, the permission, and updates.

use rune::{Shortcut, ShortcutError, Tx};

use super::state::{FormError, Login, Menu, Models, Page, State, Update, fresh_backend};
use crate::llm::{Backend, chatgpt};
use crate::settings::{Appearance, ChatGptAuth, Provider, Reasoning};
use crate::{login_item, mac, update};

impl State {
    pub fn open_settings(&mut self) {
        self.trusted = mac::accessibility_trusted(false);
        self.form.at_login = login_item::enabled();
        self.menu = None;
        self.page = Page::Settings;
        if matches!(self.form.models, Models::Idle) && Backend::of(&self.settings).is_ok() {
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
        self.save();
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
        self.trusted = mac::accessibility_trusted(true);
        if !self.trusted {
            mac::open_accessibility_settings();
        }
    }

    pub fn recheck_accessibility(&mut self) {
        self.trusted = mac::accessibility_trusted(false);
        if self.trusted && matches!(self.form.error, Some(FormError::Accessibility)) {
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
