//! What the window draws: the text at the top, the actions under it, and the
//! answer at the bottom; or the settings ([`settings_view`](super::settings_view)).

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Duration;

use rune::{
    Color, Cx, El, Icon, Key, KeyEvent, Link, LinkPaint, ScrollSpec, Span, SystemMaterial, Tx, col,
    el, icon, rich, row, spacer, text,
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Kit, MenuItem, Placement, Popover,
    Spinner, Switch, TextArea,
};

use super::buttons::{self, Tone};
use super::dictating::{Dictation, LEVELS_KEPT};
use super::draft::{Answer, Status};
use super::icons;
use super::state::{Menu, Page, State};
use super::{onboarding, settings_view};
use crate::diff::{self, Kind, Piece};
use crate::i18n::fill;
use crate::prompts::{Action, Style};

/// The window's corner radius.
pub const CORNER: f32 = 16.0;
/// The window's width until the main page's actions lay out; then it's as
/// wide as they are on one line, at least [`MIN_WIDTH`].
pub const WIDTH: f32 = 560.0;
/// The narrowest the window gets, so the settings still fit.
pub const MIN_WIDTH: f32 = 480.0;
/// The tallest the answer gets before it scrolls.
const ANSWER_MAX: f32 = 320.0;
/// The tallest the language list gets before it scrolls.
const LANGUAGES_MAX: f32 = 250.0;
/// Clear space under the window while a dropdown is open: the window is as
/// tall as its content, and a dropdown may hang below it. The language list
/// is the tallest.
const MENU_ROOM: f32 = 300.0;

/// Return the theme's kit with Fettle's violet, the app icon's colour, as the
/// accent in place of the theme's blue.
fn kit(dark: bool) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| {
        let mut k = rune_theme_opencode::kit(dark).clone();
        // Violet 500 on dark; the deeper 600 on light keeps white button text readable.
        let (fill, ink) = if dark {
            ("#8b5cf6", "#a78bfa")
        } else {
            ("#7c3aed", "#7c3aed")
        };
        let (fill, ink) = (
            rune::color::hex(fill).expect("a valid hex colour"),
            rune::color::hex(ink).expect("a valid hex colour"),
        );
        k.c.bg_accent = fill;
        k.c.text_accent = ink;
        k.c.icon_accent = ink;
        k
    })
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = s.settings.appearance.dark(cx.system_dark());
    let k = kit(dark);
    let mut layer = Vec::new();
    let page = match s.page {
        Page::Main => main(s, k, &mut layer),
        Page::Settings => settings_view::view(s, k, &mut layer),
        Page::Onboarding(step) => onboarding::view(s, k, step),
    };
    let tint = if dark {
        k.c.bg_base.alpha(0.72)
    } else {
        Color::WHITE.alpha(0.8)
    };
    // macOS draws dark windows' rims itself; a light line on top reads as a seam.
    let edge = if dark {
        Color::TRANSPARENT
    } else {
        k.c.border_base.alpha(0.6)
    };
    let card = col()
        .key("card")
        .w_full()
        .radius(CORNER)
        .system_material(SystemMaterial::Popover)
        .child(
            col()
                .w_full()
                .radius(CORNER)
                .fill(tint)
                .border(1.0, edge)
                .padding(14.0)
                .gap(12.0)
                .child(page),
        );
    // Not `rune_kit::root`: it fills the window, and the window fits the content.
    let mut root = col().key("window").w_full().child(card);
    if s.menu.is_some() {
        root = root.child(el().key("menu-room").w_full().h(MENU_ROOM));
    }
    root.children(layer)
}

fn main(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut page = col()
        .w_full()
        .gap(12.0)
        .child(header(s, k, Lead::Mark, "Fettle", main_tools(s, k)))
        .child(source(s, k));
    if let Some(notice) = &s.draft.notice {
        page = page.child(note(k, notice, NoteKind::Info));
    }
    page = page.child(actions(s, k, layer));
    if let Some(answer) = &s.draft.answer {
        page = page.child(answer_view(s, answer, k));
    }
    page
}

/// The two-row text box's height: rows of 17.5 points plus padding, as Kit's
/// text area draws it.
const SOURCE_MIN: f32 = 51.0;

/// The editable text, with buttons in its corner that dictate and clear it;
/// while dictating, the waveform stands in for it.
fn source(s: &State, k: &Kit) -> El {
    let t = s.t();
    let asking = match &s.dictation {
        Some(Dictation::Listening(recording)) => {
            let stop = IconButton::new(&icons::MIC)
                .variant(IconButtonVariant::Contrast)
                .label(t.stop_dictating)
                .on_press(|s: &mut State| s.dictate())
                .build(k);
            return recorder(&recording.levels, recording.duration(), stop, k);
        }
        Some(Dictation::Writing(write_up)) => {
            // As wide as the stop button, so nothing moves.
            let spinner = row()
                .size(24.0, 24.0)
                .center()
                .child(Spinner::new().size(14.0).build(k));
            return recorder(&write_up.levels, write_up.took, spinner, k);
        }
        Some(Dictation::Asking) => true,
        None => false,
    };
    let field = TextArea::new(&s.draft.source)
        .label(t.text)
        .placeholder(t.placeholder)
        .width(None)
        .auto_grow(2, 8)
        .on_edit(|s: &mut State| &mut s.draft.source)
        .build(k)
        // Kit tints fields under the pointer like buttons; a text box shouldn't.
        .hover(Color::TRANSPARENT);
    let mut tools = vec![
        IconButton::new(&icons::MIC)
            .variant(IconButtonVariant::GhostMuted)
            .label(t.dictate)
            .disabled(asking)
            .on_press(|s: &mut State| s.dictate())
            .build(k),
    ];
    if !s.draft.source.text.is_empty() {
        tools.push(
            IconButton::new(&icons::CLOSE)
                .variant(IconButtonVariant::GhostMuted)
                .label(t.clear)
                .on_press(|s: &mut State| s.clear_source())
                .build(k),
        );
    }
    // Room on the right so the text doesn't run under the buttons.
    let room = 8.0 + tools.len() as f32 * 26.0;
    col()
        .w_full()
        .child(field.pr(room))
        .child(
            row()
                .gap(2.0)
                .children(tools)
                .pin(None, Some(6.0), Some(6.0), None),
        )
}

/// While dictating, what stands in for the text: a waveform of the
/// microphone's `levels`, the time it `took`, and `end`: a button that
/// stops, or once listening ends, a spinner until the words land in the text.
fn recorder(levels: &VecDeque<f32>, took: Duration, end: El, k: &Kit) -> El {
    let bars = levels.iter().map(|level| {
        el().size(2.0, (2.0 + level * 17.0).round())
            .radius(1.0)
            .fill(k.c.text_accent)
    });
    let secs = took.as_secs();
    row()
        .key("recorder")
        .w_full()
        .min_h(SOURCE_MIN)
        .pl(10.0)
        .pr(6.0)
        .gap(10.0)
        .center_y()
        .radius(k.radius.md)
        .fill(k.c.bg_base)
        .outline(1.0, -1.0, k.c.border_focus)
        .child(
            // Newest on the right, the oldest fading out on the left.
            row()
                .w(LEVELS_KEPT as f32 * 4.0 - 2.0)
                .h(19.0)
                .gap(2.0)
                .center_y()
                .justify_end()
                .clip_fade_linear(24.0, 0.0, 0.0, 0.0)
                .children(bars),
        )
        .child(spacer())
        .child(
            text(format!("{}:{:02}", secs / 60, secs % 60))
                .font(k.ty.text_small)
                .color(k.c.text_faint),
        )
        .child(end)
}

/// What starts a header: Fettle's mark, or a button back.
#[derive(Debug)]
pub enum Lead {
    Mark,
    Back(fn(&mut State)),
}

/// The title row: `lead`, `title`, and on the right `tools` and the hide
/// button. It moves the window when dragged.
pub fn header(s: &State, k: &Kit, lead: Lead, title: &str, tools: Vec<El>) -> El {
    let t = s.t();
    let lead = match lead {
        Lead::Mark => icon(&icons::SPARKLE, 15.0).color(k.c.text_accent),
        Lead::Back(back) => IconButton::new(&icons::BACK)
            .variant(IconButtonVariant::GhostMuted)
            .label(t.back)
            .on_press(back)
            .build(k),
    };
    row()
        .key("header")
        .w_full()
        .center_y()
        .gap(6.0)
        .on_drag(|s: &mut State, drag, _| s.drag(drag.phase))
        .child(lead)
        .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
        .child(spacer())
        .children(tools)
        .child(
            IconButton::new(&icons::CLOSE)
                .variant(IconButtonVariant::GhostMuted)
                .label(t.hide)
                .shortcut("Esc")
                .on_press(|s: &mut State| s.hide())
                .build(k),
        )
}

/// The main page's header buttons: settings, and pinning the window open.
fn main_tools(s: &State, k: &Kit) -> Vec<El> {
    let t = s.t();
    let settings = IconButton::new(&icons::GEAR)
        .variant(IconButtonVariant::GhostMuted)
        .label(t.settings)
        .shortcut("⌘,")
        .on_press(|s: &mut State| s.open_settings())
        .build(k);
    let pin = IconButton::new(&icons::PIN)
        .variant(if s.pinned {
            IconButtonVariant::Ghost
        } else {
            IconButtonVariant::GhostMuted
        })
        .label(if s.pinned { t.unpin } else { t.pin })
        .on_press(|s: &mut State| s.toggle_pin())
        .build(k);
    let pin = if s.pinned {
        pin.fill(k.c.bg_accent.alpha(0.16))
    } else {
        pin
    };
    vec![settings, pin]
}

fn actions(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let line = row()
        .key("actions")
        .center_y()
        .gap(8.0)
        // The window takes the line's width, padded on both sides as on its left.
        .on_layout_tx(|s: &mut State, [x, _, w, _], tx| s.fit_width(w + 2.0 * x, tx))
        .child(buttons::button(
            k,
            Tone::Primary,
            t.fix,
            Some(&icons::SPARKLE),
            |b| {
                b.shortcut("⌘1")
                    .on_press(|s: &mut State| s.start(Action::Fix))
            },
        ))
        .child(translate(s, k, layer))
        .child(rewrite(s, k, layer));
    // In a row, so the line keeps its own width rather than stretching to the page's.
    row().w_full().child(line)
}

/// Rewrite and the style it writes in, joined in one control as Translate is.
fn rewrite(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let current = s.settings.rewrite_style;
    let mut styles = rune_kit::Menu::new("styles")
        .trigger(picker(k, t.style(current), t.rewrite_style, Menu::Rewrite))
        // The standard look has the raised surface and shadow the language list has.
        .standard()
        .open(s.menu_open(Menu::Rewrite))
        // The control sits at the row's end, so the menu lines up with its right edge.
        .placement(Placement::BottomEnd)
        .on_close(|s: &mut State| s.close_menu());
    for style in Style::ALL {
        // Tone first, then length and clarity.
        if style == Style::Shorter {
            styles = styles.separator();
        }
        styles = styles.item(
            MenuItem::new(t.style(style))
                .icon(style_icon(style))
                .checked(style == current)
                .on_select(move |s: &mut State| s.set_style(style)),
        );
    }
    let action = split_part(
        Button::new(t.rewrite)
            .icon(&icons::PEN)
            .shortcut("⌘3")
            .on_press(move |s: &mut State| s.start(Action::Rewrite(current))),
        k,
    );
    split(k, action, styles.build(k, layer))
}

fn style_icon(style: Style) -> &'static Icon {
    match style {
        Style::Formal => &icons::BRIEFCASE,
        Style::Friendly => &icons::SMILE,
        Style::Shorter => &icons::SHORTER,
        Style::Clearer => &icons::EYE,
    }
}

/// The part of a split control that opens `menu`, showing the choice `label`.
fn picker(k: &Kit, label: &str, help: &str, menu: Menu) -> El {
    split_part(
        Button::new(label)
            .icon(&icons::CHEV_DOWN)
            .help(help)
            .on_press(move |s: &mut State| s.toggle_menu(menu)),
        k,
    )
}

/// Build `button` as one part of a split control; the control highlights as a whole.
fn split_part(button: Button, k: &Kit) -> El {
    button
        .variant(ButtonVariant::Ghost)
        .build(k)
        .radius(14.0)
        .hover(Color::TRANSPARENT)
}

/// An action and the choice it uses, joined in one capsule with a rule between.
fn split(k: &Kit, action: El, picker: El) -> El {
    let group = row()
        .h(28.0)
        .px(2.0)
        .center_y()
        .child(action)
        .child(el().w(1.0).h(16.0).fill(k.c.border_muted))
        .child(picker);
    buttons::surface(k, Tone::Secondary, group)
}

/// Translate and the language it writes in, joined in one control.
fn translate(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let open = s.menu_open(Menu::Language);
    let mut picker = Popover::new("languages")
        .trigger(picker(
            k,
            t.language(&s.settings.language),
            t.translation_language,
            Menu::Language,
        ))
        .open(open)
        .placement(Placement::BottomStart)
        .label(t.translation_language)
        .padding(4.0)
        .on_close(|s: &mut State| s.close_menu());
    // A closed popover drops its content, so skip building the list.
    if open {
        picker = picker.child(languages(s, k));
    }
    let action = split_part(
        Button::new(t.translate)
            .icon(&icons::GLOBE)
            .shortcut("⌘2")
            .on_press(|s: &mut State| s.start(Action::Translate)),
        k,
    );
    split(k, action, picker.build(k, layer))
}

/// The languages, favourites first; a star marks or unmarks one.
fn languages(s: &State, k: &Kit) -> El {
    let (mut favourites, mut rest) = s.settings.languages();
    s.locale().sort_languages(&mut favourites);
    s.locale().sort_languages(&mut rest);
    let mut list = col().w(240.0).gap(2.0);
    if !favourites.is_empty() {
        list = list
            .children(favourites.into_iter().map(|l| language(s, k, l)))
            .child(el().w_full().h(1.0).my(3.0).fill(k.c.border_muted));
    }
    list = list.children(rest.into_iter().map(|l| language(s, k, l)));
    col()
        .max_h(LANGUAGES_MAX)
        .min_h(0.0)
        .shrink()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(list)
}

fn language(s: &State, k: &Kit, name: &'static str) -> El {
    let t = s.t();
    let favourite = s.settings.is_favourite(name);
    let current = s.settings.language == name;
    let star = el()
        .key(format!("star-{name}"))
        .role("button")
        .label(if favourite {
            t.remove_favourite
        } else {
            t.add_favourite
        })
        .size(24.0, 24.0)
        .center()
        .radius(4.0)
        .hover(k.c.hover)
        .on_press(move |s: &mut State| s.toggle_favourite(name))
        .child(
            icon(
                if favourite {
                    &icons::STAR_FILLED
                } else {
                    &icons::STAR
                },
                14.0,
            )
            .color(if favourite {
                k.c.warning.icon
            } else {
                k.c.icon_faint
            }),
        );
    row()
        .key(format!("language-{name}"))
        .role("option")
        .label(t.language(name))
        .w_full()
        .h(30.0)
        .pl(3.0)
        .pr(8.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .hover(k.c.hover)
        .on_press(move |s: &mut State| s.set_language(name))
        .child(star)
        .child(
            text(t.language(name))
                .font(k.ty.ui)
                .color(k.c.text_base)
                .grow(1.0),
        )
        .child(
            icon(k.glyph.check_small, 14.0)
                .color(k.c.icon_base)
                .opacity(if current { 1.0 } else { 0.0 }),
        )
}

fn answer_view(s: &State, answer: &Answer, k: &Kit) -> El {
    let t = s.t();
    let running = matches!(answer.status, Status::Running { .. });
    let language = t.language(&s.settings.language);
    let title = match answer.action {
        Action::Fix if running => t.fixing.to_owned(),
        Action::Fix => t.fixed.to_owned(),
        Action::Translate if running => fill(t.translating, language),
        Action::Translate => fill(t.translation, language),
        Action::Rewrite(_) if running => t.rewriting.to_owned(),
        Action::Rewrite(style) => fill(t.rewritten, t.style(style)),
    };
    // Only a Fix answer has changes.
    let changed = matches!(&answer.status, Status::Done { changes, .. } if diff::changed(changes));
    let glyph = match answer.action {
        Action::Fix => &icons::SPARKLE,
        Action::Translate => &icons::GLOBE,
        Action::Rewrite(_) => &icons::PEN,
    };
    let mut top = row()
        .w_full()
        .h(28.0)
        .center_y()
        .gap(8.0)
        .child(icon(glyph, 15.0).color(k.c.icon_accent))
        .child(text(title).font(k.ty.ui_medium).color(k.c.text_base));
    if running {
        top = top.child(Spinner::new().size(12.0).build(k));
    }
    top = top.child(spacer());
    if running {
        top = top.child(
            Button::new(t.stop)
                .icon(&icons::STOP)
                .variant(ButtonVariant::GhostMuted)
                .on_press(|s: &mut State| s.stop())
                .build(k),
        );
    }
    if changed {
        top = top.child(
            Switch::new(s.draft.show_changes)
                .label(t.show_changes)
                .on_toggle(|s: &mut State| s.toggle_changes())
                .build(k),
        );
    }
    let plain = |content: &str| {
        scroll_box(
            text(content)
                .font(k.ty.body)
                .color(k.c.text_base)
                .wrap()
                .selectable(true),
        )
    };
    let body = match &answer.status {
        Status::Failed(e) => note(k, e, NoteKind::Failure),
        Status::Stopped => note(k, s.t().stopped, NoteKind::Failure),
        Status::Running { text: partial, .. } if partial.is_empty() => {
            text(t.thinking).font(k.ty.body).color(k.c.text_faint)
        }
        Status::Running { text: partial, .. } => plain(partial),
        Status::Done { .. } if answer.action == Action::Fix && !changed => {
            note(k, t.nothing_to_fix, NoteKind::Info)
        }
        Status::Done { changes, .. } if changed && s.draft.show_changes => {
            scroll_box(diff_text(s, changes, k))
        }
        Status::Done { text: done, .. } => plain(done),
    };
    let mut section = col()
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .radius(k.radius.lg)
        .fill(k.c.bg_base.alpha(0.85))
        .border(1.0, k.c.border_muted)
        .child(top)
        .child(divider(k))
        .child(body);
    if let Status::Done { copied, .. } = answer.status {
        section = section.child(
            row()
                .w_full()
                .gap(8.0)
                .child(spacer())
                .child(buttons::button(
                    k,
                    Tone::Secondary,
                    if copied { t.copied } else { t.copy },
                    Some(if copied { &icons::CHECK } else { &icons::COPY }),
                    |b| {
                        b.shortcut("⇧⌘C")
                            .on_press_tx(|s: &mut State, tx| s.copy(tx))
                    },
                ))
                .child(buttons::button(
                    k,
                    Tone::Primary,
                    t.replace,
                    Some(&icons::REPLACE),
                    |b| {
                        b.shortcut("⌘↩")
                            .help(t.replace_help)
                            .on_press(|s: &mut State| s.replace())
                    },
                )),
        );
    }
    section
}

/// The fixed text with removed words struck in red and added ones in green.
/// A click on a change declines it.
fn diff_text(s: &State, changes: &[Piece], k: &Kit) -> El {
    let t = s.t();
    let mut content = String::new();
    let mut spans = Vec::new();
    let mut strikes = Vec::new();
    // Each change's bytes in `content`, by its number.
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut previous = Kind::Same;
    for (change, piece) in diff::numbered(changes) {
        // A thin space keeps a word from running into the one that replaces it.
        if previous == Kind::Removed && piece.kind == Kind::Added {
            content.push('\u{2009}');
        }
        previous = piece.kind;
        let start = content.len();
        content.push_str(&piece.text);
        if let Some(n) = change {
            match ranges.get_mut(n) {
                Some(range) => range.end = content.len(),
                None => ranges.push(start..content.len()),
            }
        }
        let span = Span::new(start, content.len());
        match piece.kind {
            Kind::Same => {}
            Kind::Removed => {
                strikes.push((start, content.len(), Some(k.c.danger.icon.alpha(0.6))));
                spans.push(Span {
                    color: Some(k.c.danger.fg.mix(k.c.text_muted, 0.45)),
                    bg: Some(k.c.danger.bg.alpha(0.45)),
                    bg_pad: 1.5,
                    ..span
                });
            }
            Kind::Added => spans.push(Span {
                color: Some(k.c.success.fg.mix(k.c.text_base, 0.35)),
                bg: Some(k.c.success.bg.alpha(0.55)),
                bg_pad: 1.5,
                weight: Some(520.0),
                ..span
            }),
        }
    }
    let hover = LinkPaint::new().ring(k.c.border_strong, 1.0);
    let text = ranges
        .into_iter()
        .enumerate()
        .fold(rich(content, spans), |text, (n, range)| {
            text.link(
                Link::new(range, n.to_string())
                    .role("button")
                    .help(t.decline_change)
                    .hover(hover),
            )
        });
    text.on_link(|s: &mut State, id, _| {
        if let Ok(n) = id.parse() {
            s.decline(n);
        }
    })
    .strikes(strikes)
    .font(k.ty.body)
    .color(k.c.text_base)
    .selectable(true)
}

/// `content`, scrolling past [`ANSWER_MAX`].
fn scroll_box(content: El) -> El {
    col()
        .w_full()
        .max_h(ANSWER_MAX)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}

/// What a [`note`] says, which sets how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Info,
    Failure,
}

pub fn note(k: &Kit, message: &str, kind: NoteKind) -> El {
    let mut line = row().w_full().gap(6.0);
    let color = match kind {
        NoteKind::Info => k.c.text_muted,
        NoteKind::Failure => {
            line = line.child(icon(&icons::ALERT, 14.0).color(k.c.danger.icon));
            k.c.danger.fg
        }
    };
    line.child(
        text(message.to_owned())
            .font(k.ty.text_small)
            .color(color)
            .wrap()
            .grow(1.0),
    )
}

pub fn divider(k: &Kit) -> El {
    el().w_full().h(1.0).fill(k.c.border_muted)
}

pub fn on_key(s: &mut State, key: &KeyEvent, tx: &mut Tx) -> bool {
    if key.key == Key::Escape {
        match s.page {
            _ if s.form.shortcut.is_recording() => return false,
            _ if s.any_menu() => s.close_menu(),
            Page::Settings => s.back(),
            Page::Main | Page::Onboarding(_) => s.hide(),
        }
        return true;
    }
    if key.is_command(',') {
        s.open_settings();
    } else if key.is_command('w') {
        s.hide();
    } else if key.is_command('q') {
        tx.quit();
    } else {
        return s.page == Page::Main && on_main_key(s, key, tx);
    }
    true
}

/// Handle the shortcuts for the main page's actions and answer.
fn on_main_key(s: &mut State, key: &KeyEvent, tx: &mut Tx) -> bool {
    if key.key == Key::Enter && key.mods.cmd {
        s.replace();
    } else if key.is_command('1') {
        s.start(Action::Fix);
    } else if key.is_command('2') {
        s.start(Action::Translate);
    } else if key.is_command('3') {
        s.start(Action::Rewrite(s.settings.rewrite_style));
    } else if key.is_command('c') && key.mods.shift {
        s.copy(tx);
    } else {
        return false;
    }
    true
}
