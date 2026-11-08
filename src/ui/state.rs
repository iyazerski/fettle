//! The window's state and what changes it on the main page: showing the
//! selection, running an action, and moving the window. The text and answer
//! are in [`draft`](super::draft), the settings page's part in
//! [`settings_state`](super::settings_state).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use rune::{AppHandle, Phase, ShortcutId, Tx};
use tokio::runtime::Handle;
use tokio::sync::{Mutex, oneshot};

use super::checker::{Checker, Target};
use super::draft::{Draft, Status, complete};
use super::onboarding::Step;
use super::saver::Saver;
use super::settings_state::{Form, FormError, Update};
use super::view::WIDTH;
use crate::i18n::{Locale, Strings, fill};
use crate::llm::on_device::{self, Availability, Refusal};
use crate::llm::{Ask, Backend, chatgpt};
use crate::mac;
use crate::prompts::{self, Action};
use crate::settings::{Credentials, Settings};
use crate::spelling::Mistake;

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
    /// Held while the ChatGPT sign-in refreshes, so requests take turns.
    pub(super) refreshing: Arc<Mutex<()>>,
    pub settings: Settings,
    saver: Saver,
    /// Whether Apple's on-device model can run, as last checked.
    pub on_device: Availability,
    pub page: Page,
    pub draft: Draft,
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
    /// Whether Fettle may copy and paste in other apps.
    pub trusted: bool,
    /// Whether macOS's Accessibility prompt was asked for since launch.
    pub(super) asked_accessibility: bool,
    pub form: Form,
    pub update: Update,
    pub(super) shortcut: Option<ShortcutId>,
    /// Whether the selection is being copied, so a second press can't copy
    /// over the first one's borrowed pasteboard.
    capturing: bool,
    placement: Placement,
    checker: Checker,
    /// Where the text came from when a click on an underline brought it, to
    /// paste the answer back over.
    target: Option<Target>,
}

/// Where the window is, and the drag moving it.
#[derive(Default)]
struct Placement {
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

impl Placement {
    /// Follow a drag's `phase` with the pointer at `pointer`, and return
    /// where the window moves to, if it moves.
    fn drag(&mut self, phase: Phase, pointer: Option<(f64, f64)>) -> Option<(i32, i32)> {
        match phase {
            Phase::Down => {
                self.drag = pointer
                    .zip(self.position)
                    .map(|(pointer, origin)| Drag { pointer, origin });
                None
            }
            Phase::Move => {
                let (Drag { pointer, origin }, now) = self.drag.zip(pointer)?;
                let at = (
                    origin.0 + (now.0 - pointer.0).round() as i32,
                    origin.1 + (now.1 - pointer.1).round() as i32,
                );
                self.position = Some(at);
                Some(at)
            }
            Phase::Up => {
                self.drag = None;
                None
            }
        }
    }
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
    Onboarding(Step),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Language,
    Model,
    AppLanguage,
    Rewrite,
}

impl State {
    pub fn new(
        app: AppHandle<State>,
        rt: Handle,
        http: reqwest::Client,
        settings: Settings,
        settings_path: PathBuf,
        saved_credentials: Credentials,
    ) -> Self {
        let form = Form::new(&settings);
        let saver = Saver::new(
            &rt,
            app.clone(),
            &settings,
            settings_path,
            saved_credentials,
        );
        Self {
            app,
            rt,
            http,
            refreshing: Arc::default(),
            settings,
            saver,
            on_device: on_device::availability(),
            page: Page::Main,
            draft: Draft::new(),
            pinned: false,
            system_locale: Locale::from_preferred(&mac::preferred_languages()),
            menu: None,
            menu_shown: false,
            trusted: mac::accessibility_trusted(),
            asked_accessibility: false,
            form,
            update: Update::Idle,
            shortcut: None,
            capturing: false,
            placement: Placement::default(),
            checker: Checker::default(),
            target: None,
        }
    }

    /// Take the shortcut, and show the welcome steps on the first run, or the
    /// settings if `start` says so.
    pub fn started(&mut self, start: Start) {
        self.apply_appearance();
        self.register_saved_shortcut();
        self.apply_underlines();
        // Rewrite the file without the credentials versions before 0.3 kept in it.
        self.save();
        eprintln!(
            "fettle {} is running; {} opens it",
            crate::VERSION,
            self.settings.shortcut
        );
        if !self.settings.onboarded {
            self.open_onboarding();
        } else if start == Start::Shown {
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
        if !self.settings.onboarded {
            self.open_onboarding();
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

    /// Underline mistakes in other apps if the settings say so, else stop.
    pub(super) fn apply_underlines(&mut self) {
        if self.settings.underline_mistakes {
            self.checker.start(&self.app, &self.rt);
        } else {
            self.checker.stop();
        }
    }

    /// Underline the mistakes in the focused field, unless the window shows.
    pub fn check_field(&mut self) {
        if self.app.is_visible() || self.capturing {
            self.checker.pause();
        } else {
            self.checker.refresh();
        }
    }

    /// The answer to the field check `id`.
    pub fn field_checked(&mut self, id: u64, found: Vec<Mistake>) {
        self.checker.finished(id, found);
    }

    /// A click at `point` in another app: on an underline, open its paragraph and fix it.
    pub fn underline_clicked(&mut self, point: (f64, f64)) {
        if self.app.is_visible() || self.capturing {
            return;
        }
        let Some(target) = self.checker.clicked(point) else {
            return;
        };
        self.show_text(Some(target.text.clone()));
        self.target = Some(target);
        self.start(Action::Fix);
    }

    fn show_text(&mut self, text: Option<String>) {
        self.capturing = false;
        self.target = None;
        self.stop();
        self.page = Page::Main;
        self.menu = None;
        let notice = text.is_none().then(|| self.t().no_selection.into());
        self.draft.reset(text.unwrap_or_default(), notice);
        self.show();
    }

    pub(super) fn show(&mut self) {
        // Not `AppHandle::place`: it pins the window's size, and the window fits its content.
        if !self.app.is_visible()
            && let Some(position) = mac::top_center_under_pointer(WIDTH, TOP_MARGIN)
        {
            self.placement.position = Some(position);
            self.app.set_position(position);
        }
        self.app.open();
    }

    pub fn hide(&mut self) {
        self.stop();
        self.menu = None;
        self.app.hide();
    }

    /// The window lost focus: hide it, unless it's pinned or shows the
    /// welcome steps, which a new user can't open again without the shortcut.
    pub fn blurred(&mut self) {
        let onboarding = matches!(self.page, Page::Onboarding(_));
        if !self.pinned && !onboarding && !self.form.shortcut.is_recording() {
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

    pub fn toggle_changes(&mut self) {
        self.draft.show_changes = !self.draft.show_changes;
    }

    pub fn clear_source(&mut self) {
        self.draft.source.replace_text(String::new());
    }

    /// Move the window with the pointer. The panel is outside winit's reach, so
    /// `.window_drag()` can't move it.
    pub fn drag(&mut self, phase: Phase) {
        if let Some(at) = self.placement.drag(phase, mac::pointer()) {
            self.app.set_position(at);
        }
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
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
        let t = self.t();
        if self.draft.source.text.trim().is_empty() {
            self.draft.notice = Some(t.no_text.into());
            return;
        }
        self.draft.notice = None;
        let run = self.draft.cancel(t.stopped);
        let (backend, model) = match Backend::with_model(&self.settings) {
            Ok(chosen) => chosen,
            Err(missing) => {
                self.draft.fail(action, t.missing(missing).into());
                return;
            }
        };
        let ask = Ask {
            system: prompts::system(action, &self.settings.language),
            text: self.draft.source.text.clone(),
            model,
            effort: self.settings.effort(),
        };
        let (app, http, refreshing) =
            (self.app.clone(), self.http.clone(), self.refreshing.clone());
        let job = self.rt.spawn(async move {
            let mut streamed = String::new();
            let result = async {
                let backend = fresh_backend(&app, &http, &refreshing, backend).await?;
                backend
                    .stream(&http, &ask, |delta| {
                        streamed.push_str(delta);
                        let delta = delta.to_owned();
                        app.update(move |s, _| s.draft.delta(run, &delta));
                    })
                    .await
            }
            .await;
            let status = match result {
                // The diff can take a while on a long text: keep it off the UI thread.
                Ok(()) => tokio::task::spawn_blocking(move || {
                    complete(action, &ask.text, &streamed, t.empty_answer)
                })
                .await
                .unwrap_or_else(|e| Status::Failed(e.to_string())),
                Err(e) => Status::Failed(match e.downcast_ref::<Refusal>() {
                    Some(refusal) => t.refusal(*refusal).into(),
                    None => format!("{e:#}"),
                }),
            };
            app.update(move |s, _| s.draft.finished(run, status));
        });
        self.draft.running(action, run, job);
    }

    /// Stop the running answer, if any.
    pub fn stop(&mut self) {
        self.draft.cancel(self.t().stopped);
    }

    /// Hide the window and paste the answer over the selection.
    pub fn replace(&mut self) {
        let Some(text) = self.draft.result().map(str::to_owned) else {
            return;
        };
        if let Some(target) = &self.target
            && !target.select()
        {
            self.draft.notice = Some(self.t().text_moved.into());
            return;
        }
        self.hide();
        self.rt.spawn_blocking(move || {
            std::thread::sleep(PASTE_DELAY);
            mac::paste(&text);
        });
    }

    pub fn copy(&mut self) {
        let Some(text) = self.draft.result() else {
            return;
        };
        mac::copy(text);
        self.draft.copied();
    }

    pub fn set_language(&mut self, language: &str) {
        self.menu = None;
        self.settings.language = language.to_owned();
        self.save();
        if self.draft.translated() {
            self.start(Action::Translate);
        }
    }

    pub fn toggle_favourite(&mut self, language: &str) {
        self.settings.toggle_favourite(language);
        self.save();
    }

    pub(super) fn save(&self) {
        self.saver.save(&self.settings);
    }

    pub(super) fn save_failed(&mut self, error: &anyhow::Error) {
        self.report(fill(self.t().save_failed, format_args!("{error:#}")));
    }

    /// Quit, saving what's unsaved first.
    pub fn quit(&mut self, tx: &mut Tx) {
        if let Err(e) = self.saver.flush() {
            eprintln!("saving the settings: {e:#}");
        }
        tx.quit();
    }

    /// Show `message` on the page that's open.
    pub(super) fn report(&mut self, message: String) {
        match self.page {
            Page::Main | Page::Onboarding(_) => self.draft.notice = Some(message),
            Page::Settings => self.form.error = Some(FormError::Other(message)),
        }
    }
}

/// Return `backend` with a fresh ChatGPT token, refreshing and saving it if it
/// expires soon.
///
/// A refresh token works once, so refreshes take turns on `refreshing`, and
/// each starts from the sign-in the settings hold by then.
pub(super) async fn fresh_backend(
    app: &AppHandle<State>,
    http: &reqwest::Client,
    refreshing: &Mutex<()>,
    backend: Backend,
) -> anyhow::Result<Backend> {
    let Backend::ChatGpt(auth) = &backend else {
        return Ok(backend);
    };
    if !chatgpt::expiring(auth) {
        return Ok(backend);
    }
    let _turn = refreshing.lock().await;
    let (tx, rx) = oneshot::channel();
    app.update(move |s, _| {
        let _ = tx.send(s.settings.credentials.chatgpt.clone());
    });
    let auth = rx
        .await
        .ok()
        .flatten()
        .context("signed out of ChatGPT: sign in again in Settings")?;
    // The request before this one may have refreshed it already.
    if !chatgpt::expiring(&auth) {
        return Ok(Backend::ChatGpt(auth));
    }
    let fresh = chatgpt::refresh(http, &auth).await?;
    let saved = fresh.clone();
    // Queued before the turn ends, so the next request reads the refreshed sign-in.
    app.update(move |s, _| s.saved_auth(&auth.refresh, saved));
    Ok(Backend::ChatGpt(fresh))
}
