//! The window's state and what changes it on the main page: showing the
//! selection, running an action, and moving the window. The text and answer
//! are in [`draft`](super::draft), the settings page's part in
//! [`settings_state`](super::settings_state).

use std::path::PathBuf;
use std::time::Duration;

use objc2_core_foundation::CGRect;
use rune::{AppHandle, Label, Phase, ShortcutId};
use tokio::runtime::Handle;

use super::checker::{Check, Checker, Fix, Target};
use super::client::Client;
use super::dictating::Dictation;
use super::draft::{Answer, Draft, Status, complete};
use super::onboarding::Step;
use super::pending::{Pending, Ticket};
use super::saver::Saver;
use super::settings_state::{Form, FormError, Update};
use super::view::WIDTH;
use crate::i18n::{Locale, Strings, fill};
use crate::llm::on_device::{self, Availability, Refusal};
use crate::llm::{Ask, Backend};
use crate::mac;
use crate::prompts::{self, Action};
use crate::settings::{Credentials, Settings};
use crate::underlines::Line;

/// How far below the top of the screen the window opens.
const TOP_MARGIN: f32 = 120.0;
/// How long the window waits, hidden, before pasting, so the app the text
/// came from has its key window back.
const PASTE_DELAY: Duration = Duration::from_millis(120);
/// How long a dropdown's list waits for the window to grow for it: a few frames.
const MENU_DELAY: Duration = Duration::from_millis(50);
/// How far the window opened from an underline sits from its word, in points.
const WORD_GAP: f64 = 6.0;

pub struct State {
    pub(super) app: AppHandle<State>,
    pub(super) client: Client,
    pub(super) settings: Settings,
    saver: Saver,
    /// Whether Apple's on-device model can run, as last checked.
    pub(super) on_device: Availability,
    pub(super) page: Page,
    pub(super) draft: Draft,
    pub(super) dictation: Option<Dictation>,
    /// Keep the window open when another app takes focus.
    pub(super) pinned: bool,
    /// The language macOS prefers, of those the window speaks.
    system_locale: Locale,
    /// The dropdown that's open.
    pub(super) menu: Option<Dropdown>,
    /// Whether Fettle may copy and paste in other apps.
    pub(super) trusted: bool,
    /// Whether macOS's Accessibility prompt was asked for since launch.
    pub(super) asked_accessibility: bool,
    pub(super) form: Form,
    pub(super) update: Update,
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

/// An open dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dropdown {
    pub(super) menu: Menu,
    /// This opening of it, so the wait for an earlier one can't show its list.
    opened: Ticket,
    /// Whether its list shows yet: it waits a few frames for the window to
    /// grow, since a window that fits its content doesn't lay out again when
    /// its height changes, and the list would be cut to the old height.
    pub(super) shown: bool,
}

/// Where the window is, and the drag moving it.
#[derive(Default)]
struct Placement {
    /// The window's top-left in physical pixels, once Fettle has placed it.
    position: Option<(i32, i32)>,
    /// Where a drag of the window began.
    drag: Option<Drag>,
    /// The word the window sits over until dragged, in points from the
    /// primary screen's top-left: the underline it opened from.
    over: Option<CGRect>,
    /// The window's height in points, as last laid out.
    height: f32,
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
                self.over = None;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Show the settings.
    Shown,
    /// Stay hidden until the shortcut (how it runs at login).
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Settings,
    Onboarding(Step),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            client: Client::new(app.clone(), rt, http),
            app,
            settings,
            saver,
            on_device: on_device::availability(),
            page: Page::Main,
            draft: Draft::new(),
            dictation: None,
            pinned: false,
            system_locale: Locale::from_preferred(&mac::preferred_languages()),
            menu: None,
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
        self.apply_labels();
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

    /// Name the items of Rune's own menus, a text field's Cut, Copy and Paste,
    /// in the window's language.
    pub(super) fn apply_labels(&self) {
        let t = self.t();
        rune::set_labels(move |label| match label {
            Label::Cut => Some(t.cut.to_owned()),
            Label::Copy => Some(t.copy.to_owned()),
            Label::Paste => Some(t.paste.to_owned()),
            _ => None,
        });
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
        self.client.rt().spawn_blocking(move || {
            let text = mac::copy_selection();
            app.update(move |s, _| s.show_text(text, None));
        });
    }

    /// Underline mistakes in other apps if the settings say so, else stop.
    pub(super) fn apply_underlines(&mut self) {
        if self.settings.underline_mistakes {
            self.checker.start(&self.app);
        } else {
            self.checker.stop();
        }
    }

    /// Underline `lines`, which checker `id` found.
    pub fn show_underlines(&mut self, id: u64, lines: Vec<Line>) {
        self.checker.show(id, lines);
    }

    /// The model's `fix` of the field's paragraph `units`, asked as check `id`.
    pub fn field_checked(&mut self, id: u64, units: Vec<u16>, fix: Option<Fix>) {
        self.checker.finished(id, units, fix);
    }

    /// The model or where it runs changed: fixes from the old one no longer count.
    pub(super) fn model_changed(&mut self) {
        self.checker.forget();
    }

    /// Ask the model to fix a paragraph of the focused field, as the Fix
    /// action does. Without a model to ask, the field's edits go unchecked.
    pub fn proofread(&mut self, check: Check) {
        let Check { id, units, text } = check;
        let Ok((backend, model)) = Backend::with_model(&self.settings) else {
            self.checker.skip();
            return;
        };
        let ask = Ask {
            system: prompts::system(Action::Fix, ""),
            text,
            model,
            effort: self.settings.effort(),
        };
        let client = self.client.clone();
        let request = Pending::spawn(&self.client, |_| async move {
            let result = client.answer(backend, &ask).await;
            let text = ask.text;
            let fix = match result.map(|fixed| complete(Action::Fix, &text, &fixed, "")) {
                Ok(Status::Done { text, changes, .. }) => Some(Fix { text, changes }),
                Ok(_) => None,
                Err(e) => {
                    eprintln!("checking a paragraph: {e:#}");
                    None
                }
            };
            client
                .app()
                .update(move |s, _| s.field_checked(id, units, fix));
        });
        self.checker.asking(id, request);
    }

    /// A click at `point` in another app: on an underline, open its paragraph.
    pub fn underline_clicked(&mut self, point: (f64, f64)) {
        if !self.app.is_visible() && !self.capturing {
            self.checker.clicked(point);
        }
    }

    /// Show the paragraph `target` opened from an underline, with the
    /// model's `fix` of it, or fix it now if that's not known.
    pub fn underline_opened(&mut self, target: Target, fix: Option<Fix>) {
        if self.app.is_visible() || self.capturing {
            return;
        }
        self.show_text(Some(target.text.clone()), Some(target.word));
        match fix {
            Some(fix) => self.draft.answered(
                Action::Fix,
                Status::Done {
                    text: fix.text,
                    changes: fix.changes,
                    copied: false,
                },
            ),
            None => self.start(Action::Fix),
        }
        self.target = Some(target);
    }

    /// Show `text` on the main page, over `word` if given, else at the top of the screen.
    fn show_text(&mut self, text: Option<String>, word: Option<CGRect>) {
        self.placement.over = word;
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
        let place = || match self.placement.over {
            Some(word) => mac::over_word(word, WIDTH, self.placement.height, WORD_GAP),
            None => mac::top_center_under_pointer(WIDTH, TOP_MARGIN),
        };
        if !self.app.is_visible()
            && let Some(position) = place()
        {
            self.placement.position = Some(position);
            self.app.set_position(position);
        }
        self.app.open();
    }

    /// The window was laid out `height` points tall: keep it over its word.
    pub fn laid_out(&mut self, height: f32) {
        self.placement.height = height;
        // A dropdown grows the window for its list; the window stays put for it.
        if self.menu.is_some() || !self.app.is_visible() {
            return;
        }
        if let Some(word) = self.placement.over
            && let Some(position) = mac::over_word(word, WIDTH, height, WORD_GAP)
            && self.placement.position != Some(position)
        {
            self.placement.position = Some(position);
            self.app.set_position(position);
        }
    }

    pub fn hide(&mut self) {
        self.placement.over = None;
        self.stop();
        // Dropping the session stops listening and drops what wasn't added yet.
        self.dictation = None;
        self.menu = None;
        self.app.hide();
    }

    /// The window lost focus: hide it, unless it's pinned, shows the welcome
    /// steps, which a new user can't open again without the shortcut, or macOS
    /// is asking to allow dictation.
    pub fn blurred(&mut self) {
        let onboarding = matches!(self.page, Page::Onboarding(_));
        let asking = matches!(self.dictation, Some(Dictation::Asking));
        if !self.pinned && !onboarding && !asking && !self.form.shortcut.is_recording() {
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
        if self.menu_is(menu) {
            self.menu = None;
            return;
        }
        // Grow at once rather than on a spring, so the list waits only for
        // the frame that grows the window.
        let opened = Ticket::new();
        self.app.update(move |s, tx| {
            tx.instant();
            s.menu = Some(Dropdown {
                menu,
                opened,
                shown: false,
            });
        });
        let app = self.app.clone();
        self.client.spawn(async move {
            tokio::time::sleep(MENU_DELAY).await;
            app.update(move |s, _| {
                if let Some(open) = &mut s.menu
                    && open.opened == opened
                {
                    open.shown = true;
                }
            });
        });
    }

    /// Whether `menu` is the dropdown that's open.
    pub fn menu_is(&self, menu: Menu) -> bool {
        self.menu.is_some_and(|open| open.menu == menu)
    }

    /// Whether `menu`'s list shows.
    pub fn menu_open(&self, menu: Menu) -> bool {
        self.menu
            .is_some_and(|open| open.menu == menu && open.shown)
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
        let run = self.draft.cancel();
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
        let client = self.client.clone();
        let job = self.client.spawn(async move {
            let app = client.app();
            let mut streamed = String::new();
            let result = client
                .stream(backend, &ask, |delta| {
                    streamed.push_str(delta);
                    let delta = delta.to_owned();
                    app.update(move |s, _| s.draft.delta(run, &delta));
                })
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
            app.update(move |s, _| s.run_finished(run, status));
        });
        self.draft.running(action, run, job);
    }

    /// End run `run` with `status`. A fix of a clicked paragraph also counts
    /// as its check, so the paragraph isn't sent to the model again.
    fn run_finished(&mut self, run: u64, status: Status) {
        self.draft.finished(run, status);
        if let Some(target) = &self.target
            && self.draft.source.text == target.text
            && let Some(Answer {
                action: Action::Fix,
                status: Status::Done { text, changes, .. },
            }) = &self.draft.answer
        {
            let fix = Fix {
                text: text.clone(),
                changes: changes.clone(),
            };
            self.checker.learn(target, fix);
        }
    }

    /// Stop the running answer, if any.
    pub fn stop(&mut self) {
        self.draft.cancel();
    }

    /// Hide the window and paste the answer over the selection, or over the
    /// paragraph an underline opened once it's selected again.
    pub fn replace(&mut self) {
        if self.draft.result().is_none() {
            return;
        }
        match &self.target {
            Some(target) if self.checker.select(target) => {}
            Some(_) => self.draft.notice = Some(self.t().text_moved.into()),
            None => self.paste(),
        }
    }

    /// Paste the answer once the checker selected the paragraph it came
    /// from, or say the text moved if it couldn't.
    pub fn target_selected(&mut self, selected: bool) {
        if self.target.is_none() || !self.app.is_visible() {
            return;
        }
        if selected {
            self.paste();
        } else {
            self.draft.notice = Some(self.t().text_moved.into());
        }
    }

    /// Hide the window and paste the answer into the app in front.
    fn paste(&mut self) {
        let Some(text) = self.draft.result().map(str::to_owned) else {
            return;
        };
        self.hide();
        self.client.rt().spawn_blocking(move || {
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

    /// Save what's unsaved now: Fettle is quitting, and the saving task
    /// won't get to it.
    pub fn save_now(&self) {
        if let Err(e) = self.saver.flush() {
            eprintln!("saving the settings: {e:#}");
        }
    }

    /// Show `message` on the page that's open.
    pub(super) fn report(&mut self, message: String) {
        match self.page {
            Page::Main | Page::Onboarding(_) => self.draft.notice = Some(message),
            Page::Settings => self.form.error = Some(FormError::Other(message)),
        }
    }
}
