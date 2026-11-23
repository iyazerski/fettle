use std::path::PathBuf;
use std::time::Duration;

use objc2_core_foundation::CGRect;
use rune::{AppHandle, Label, Phase, ShortcutId, Tx};
use tokio::runtime::Handle;

use super::checker::{Check, Checker, Fix, Target};
use super::client::Client;
use super::dictating::Dictation;
use super::draft::{Answer, Draft, Status, complete};
use super::menu_bar::MenuBar;
use super::onboarding::Step;
use super::pending::{Pending, Ticket};
use super::saver::Saver;
use super::settings_state::{Form, FormError, Update};
use super::view::{MIN_WIDTH, WIDTH};
use crate::diff::Kind;
use crate::i18n::{Locale, Strings, fill};
use crate::llm::on_device::{self, Availability, Refusal};
use crate::llm::{Ask, Backend};
use crate::mac::{self, Clip, Copied};
use crate::prompts::{self, Action, Style};
use crate::rich;
use crate::settings::{Credentials, Settings};
use crate::underlines::Line;

const TOP_MARGIN: f32 = 120.0;
/// Let the source app regain its key window before pasting.
pub(super) const PASTE_DELAY: Duration = Duration::from_millis(120);
/// Allow a few frames for the window to grow before showing the list.
const MENU_DELAY: Duration = Duration::from_millis(50);
/// Rune-kit's 0.25 s close spring, plus time to settle.
const MENU_FADE: Duration = Duration::from_millis(300);
/// How far the window opened from an underline sits from its word, in points.
const WORD_GAP: f64 = 6.0;

pub struct State {
    pub(super) app: AppHandle<State>,
    pub(super) client: Client,
    pub(super) settings: Settings,
    saver: Saver,
    pub(super) on_device: Availability,
    pub(super) page: Page,
    pub(super) draft: Draft,
    pub(super) dictation: Option<Dictation>,
    pub(super) pinned: bool,
    system_locale: Locale,
    pub(super) menu: Option<Dropdown>,
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
    pub(super) width: f32,
    checker: Checker,
    menu_bar: Option<MenuBar>,
    origin: Option<Origin>,
}

enum Origin {
    Selection(Copied),
    Underline(Target),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dropdown {
    pub(super) menu: Menu,
    /// This opening of it, so the wait for an earlier one can't show its list.
    opened: Ticket,
    stage: Stage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Wait for the window to grow: content-fitting windows don't relayout on height changes.
    Growing,
    Shown,
    /// Keep the list's room during fade-out so it doesn't flip above its trigger.
    Closing,
}

#[derive(Default)]
struct Placement {
    /// The window's top-left in physical pixels, once Fettle has placed it.
    position: Option<(i32, i32)>,
    drag: Option<Drag>,
    /// Underlined word, in points from the primary screen's top-left; cleared on drag.
    over: Option<CGRect>,
    /// The window's width and height in points, as last laid out.
    size: (f32, f32),
}

#[derive(Clone, Copy)]
struct Drag {
    pointer: (f64, f64),
    origin: (i32, i32),
}

impl Placement {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
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
            width: WIDTH,
            checker: Checker::default(),
            menu_bar: None,
            origin: None,
        }
    }

    pub fn started(&mut self, start: Start) {
        self.apply_appearance();
        self.apply_labels();
        self.register_saved_shortcut();
        self.apply_underlines();
        self.apply_menu_bar();
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

    pub(super) fn apply_labels(&self) {
        let t = self.t();
        rune::set_labels(move |label| match label {
            Label::Cut => Some(t.cut.to_owned()),
            Label::Copy => Some(t.copy.to_owned()),
            Label::Paste => Some(t.paste.to_owned()),
            _ => None,
        });
        if let Some(menu_bar) = &self.menu_bar {
            menu_bar.set_labels(t);
        }
    }

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
        self.client.rt().spawn_blocking(move || {
            let copied = mac::copy_selection();
            app.update(move |s, _| s.show_text(copied.map(Origin::Selection)));
        });
    }

    pub fn open_from_menu_bar(&mut self) {
        if self.app.is_visible() {
            self.app.focus();
        } else {
            self.summon();
        }
    }

    pub(super) fn apply_menu_bar(&mut self) {
        if !self.settings.menu_bar {
            self.menu_bar = None;
        } else if self.menu_bar.is_none() {
            self.menu_bar = MenuBar::new(self.app.clone(), self.t());
        }
    }

    pub(super) fn apply_underlines(&mut self) {
        if self.settings.underline_mistakes {
            self.checker.start(&self.app);
        } else {
            self.checker.stop();
        }
    }

    pub fn show_underlines(&mut self, id: u64, lines: Vec<Line>) {
        self.checker.show(id, lines);
    }

    pub fn field_checked(&mut self, id: u64, units: Vec<u16>, fix: Option<Fix>) {
        self.checker.finished(id, units, fix);
    }

    pub(super) fn model_changed(&mut self) {
        self.checker.forget();
    }

    pub fn proofread(&mut self, check: Check) {
        let Check { id, units, text } = check;
        let Ok((backend, model)) = Backend::with_model(&self.settings) else {
            self.checker.skip(id);
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

    /// A window opened from an underline leaves the field its keys, so it gets no blur
    /// when the user clicks elsewhere: the click hides it instead.
    pub fn clicked_elsewhere(&mut self, point: (f64, f64)) {
        if self.app.is_visible() {
            self.blurred();
        } else if !self.capturing {
            self.checker.clicked(point);
        }
    }

    pub fn underline_opened(&mut self, target: Target, fix: Option<Fix>) {
        if self.app.is_visible() || self.capturing {
            return;
        }
        self.show_text(Some(Origin::Underline(target)));
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
    }

    fn show_text(&mut self, origin: Option<Origin>) {
        // The origin keeps the text as it was, which the answer goes back over.
        let (text, word) = match &origin {
            Some(Origin::Selection(copied)) => (copied.text.clone(), None),
            Some(Origin::Underline(target)) => (target.text.clone(), Some(target.word)),
            None => (String::new(), None),
        };
        self.placement.over = word;
        self.capturing = false;
        self.origin = origin;
        self.stop();
        self.page = Page::Main;
        self.close_menu();
        self.draft.reset(text);
        self.show();
    }

    pub(super) fn show(&mut self) {
        // Not `AppHandle::place`: it pins the window's size, and the window fits its content.
        let place = || match self.placement.over {
            Some(word) => mac::over_word(word, self.width, self.placement.size.1, WORD_GAP),
            None => mac::top_center_under_pointer(self.width, TOP_MARGIN),
        };
        if !self.app.is_visible()
            && let Some(position) = place()
        {
            self.placement.position = Some(position);
            self.app.set_position(position);
        }
        // Over an underline, the field keeps its keys so the user can fix the text by hand.
        if self.placement.over.is_some() {
            self.app.show();
        } else {
            self.app.open();
        }
    }

    pub fn laid_out(&mut self, size: (f32, f32), tx: &mut Tx) {
        self.placement.size = size;
        // Also here, so a width the window didn't take is asked for again.
        self.resize(tx);
        // A dropdown grows the window for its list; the window stays put for it.
        if self.menu.is_some() || !self.app.is_visible() {
            return;
        }
        if let Some(word) = self.placement.over
            && let Some(position) = mac::over_word(word, size.0, size.1, WORD_GAP)
            && self.placement.position != Some(position)
        {
            self.placement.position = Some(position);
            self.app.set_position(position);
        }
    }

    pub fn fit_width(&mut self, width: f32, tx: &mut Tx) {
        self.width = width.ceil().max(MIN_WIDTH);
        self.resize(tx);
    }

    fn resize(&self, tx: &mut Tx) {
        if (self.width - self.placement.size.0).abs() > 0.5 {
            tx.resize_window_instant((self.width, self.placement.size.1));
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

    /// Stay open while pinned, during onboarding and shortcut recording, and while macOS asks
    /// to allow dictation.
    pub fn blurred(&mut self) {
        let onboarding = matches!(self.page, Page::Onboarding(_));
        let asking = matches!(self.dictation, Some(Dictation::Asking));
        if !self.pinned && !onboarding && !asking && !self.form.shortcut.is_recording() {
            self.hide();
        }
    }

    pub fn t(&self) -> &'static Strings {
        self.locale().strings()
    }

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

    /// The panel is outside winit's reach, so `.window_drag()` can't move it.
    pub fn drag(&mut self, phase: Phase) {
        if let Some(at) = self.placement.drag(phase, mac::pointer()) {
            self.app.set_position(at);
        }
    }

    pub fn close_menu(&mut self) {
        let Some(open) = &mut self.menu else {
            return;
        };
        if open.stage == Stage::Closing {
            return;
        }
        open.stage = Stage::Closing;
        let opened = open.opened;
        let app = self.app.clone();
        self.client.spawn(async move {
            tokio::time::sleep(MENU_FADE).await;
            app.update(move |s, _| {
                if s.menu.is_some_and(|open| open.opened == opened) {
                    s.menu = None;
                }
            });
        });
    }

    pub fn toggle_menu(&mut self, menu: Menu) {
        if self.menu_is(menu) {
            self.close_menu();
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
                stage: Stage::Growing,
            });
        });
        let app = self.app.clone();
        self.client.spawn(async move {
            tokio::time::sleep(MENU_DELAY).await;
            app.update(move |s, _| {
                if let Some(open) = &mut s.menu
                    && open.opened == opened
                    && open.stage == Stage::Growing
                {
                    open.stage = Stage::Shown;
                }
            });
        });
    }

    pub fn menu_is(&self, menu: Menu) -> bool {
        self.menu
            .is_some_and(|open| open.menu == menu && open.stage != Stage::Closing)
    }

    pub fn any_menu(&self) -> bool {
        self.menu.is_some_and(|open| open.stage != Stage::Closing)
    }

    pub fn menu_open(&self, menu: Menu) -> bool {
        self.menu
            .is_some_and(|open| open.menu == menu && open.stage == Stage::Shown)
    }

    pub fn start(&mut self, action: Action) {
        self.close_menu();
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
                self.draft
                    .answered(action, Status::Failed(t.missing(missing).into()));
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

    /// Cache a clicked paragraph's fix so the checker doesn't send it again.
    fn run_finished(&mut self, run: u64, status: Status) {
        self.draft.finished(run, status);
        if let Some(Origin::Underline(target)) = &self.origin
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

    pub fn stop(&mut self) {
        self.draft.cancel();
    }

    pub fn replace(&mut self) {
        let Some(text) = self.draft.result() else {
            return;
        };
        match &self.origin {
            Some(Origin::Underline(target)) if self.checker.locate(target) => {}
            Some(Origin::Underline(_)) => self.draft.notice = Some(self.t().text_moved.into()),
            Some(Origin::Selection(_)) | None => {
                let text = text.to_owned();
                let copied = match self.origin.take() {
                    Some(Origin::Selection(copied)) => Some(copied),
                    _ => None,
                };
                self.hide();
                self.client.rt().spawn_blocking(move || {
                    let clip = match copied {
                        Some(copied) => rich::replacement(&copied.clip, &copied.text, &text),
                        None => Clip::plain(&text),
                    };
                    std::thread::sleep(PASTE_DELAY);
                    mac::paste(&clip);
                });
            }
        }
    }

    pub fn target_located(&mut self, opened: Ticket, found: bool) {
        let current = matches!(&self.origin, Some(Origin::Underline(t)) if t.opened == opened);
        if !current || !self.app.is_visible() {
            return;
        }
        if !found {
            self.draft.notice = Some(self.t().text_moved.into());
            return;
        }
        let Some(text) = self.draft.result().map(str::to_owned) else {
            return;
        };
        if let Some(fixed) = self.draft.fixed() {
            self.checker.learn_fine(fixed);
        }
        self.hide();
        self.checker.replace(text);
    }

    pub fn keep(&mut self, n: usize, side: Kind) {
        self.draft.keep(n, side);
    }

    pub fn copy(&mut self, tx: &mut Tx) {
        let Some(text) = self.draft.result() else {
            return;
        };
        tx.copy(text);
        self.draft.copied();
    }

    pub fn set_language(&mut self, language: &str) {
        self.close_menu();
        self.settings.language = language.to_owned();
        self.save();
        if self.draft.translated() {
            self.start(Action::Translate);
        }
    }

    pub fn set_style(&mut self, style: Style) {
        self.settings.rewrite_style = style;
        self.save();
        self.start(Action::Rewrite(style));
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

    /// Flush before the runtime stops, undo Accessibility changes, and let a paste put the
    /// user's clipboard back before exit.
    pub fn quitting(&mut self) {
        if let Err(e) = self.saver.flush() {
            eprintln!("saving the settings: {e:#}");
        }
        self.checker.quit();
        mac::keep_pasteboard();
    }

    pub(super) fn report(&mut self, message: String) {
        match self.page {
            Page::Main | Page::Onboarding(_) => self.draft.notice = Some(message),
            Page::Settings => self.form.error = Some(FormError::Other(message)),
        }
    }
}
