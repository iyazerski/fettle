//! What the window draws: the text at the top, the actions under it, and the
//! answer at the bottom; or the settings ([`settings_view`](super::settings_view)).

use std::sync::OnceLock;

use rune::{
    Color, Cx, El, Key, KeyEvent, ScrollSpec, Span, SystemMaterial, Tx, col, el, icon, rich, row,
    spacer, text,
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Kit, Placement, Popover, Spinner, Switch,
    TextArea,
};

use super::buttons::{self, Tone};
use super::icons;
use super::settings_view;
use super::state::{Answer, Menu, Page, State, Status};
use crate::diff::{self, Kind, Piece};
use crate::i18n::fill;
use crate::prompts::{Action, Style};

/// The window's corner radius.
pub const CORNER: f32 = 16.0;
pub const WIDTH: f32 = 520.0;
/// The tallest the answer gets before it scrolls.
const ANSWER_MAX: f32 = 320.0;
/// The tallest the language list gets before it scrolls.
const LANGUAGES_MAX: f32 = 250.0;
/// Clear space under the window while a dropdown is open: the window is as
/// tall as its content, and a dropdown may hang below it.
const MENU_ROOM: f32 = LANGUAGES_MAX + 16.0;

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
        .child(header(s, k, "Fettle", None))
        .child(source(s, k));
    if let Some(notice) = &s.notice {
        page = page.child(note(k, notice, NoteKind::Info));
    }
    page = page.child(actions(s, k, layer));
    if let Some(answer) = &s.answer {
        page = page.child(answer_view(s, answer, k));
    }
    page
}

/// The editable text, with a button in its corner that clears it.
fn source(s: &State, k: &Kit) -> El {
    let t = s.t();
    let field = TextArea::new(&s.source)
        .label(t.text)
        .placeholder(t.placeholder)
        .width(None)
        .auto_grow(2, 8)
        .on_edit(|s: &mut State| &mut s.source)
        .build(k)
        // Kit tints fields under the pointer like buttons; a text box shouldn't.
        .hover(Color::TRANSPARENT);
    if s.source.text.is_empty() {
        return field;
    }
    let clear = IconButton::new(&icons::CLOSE)
        .variant(IconButtonVariant::GhostMuted)
        .label(t.clear)
        .on_press(|s: &mut State| s.clear_source())
        .build(k)
        .pin(None, Some(6.0), Some(6.0), None);
    // Room on the right so the text doesn't run under the button.
    col().w_full().child(field.pr(34.0)).child(clear)
}

/// The title row, with a back button on sub-pages and the hide button. It
/// moves the window when dragged.
pub fn header(s: &State, k: &Kit, title: &str, back: Option<fn(&mut State)>) -> El {
    let t = s.t();
    let mut bar = row()
        .key("header")
        .w_full()
        .center_y()
        .gap(6.0)
        .on_drag(|s: &mut State, drag, _| s.drag(drag.phase));
    if let Some(back) = back {
        bar = bar.child(
            IconButton::new(&icons::BACK)
                .variant(IconButtonVariant::GhostMuted)
                .label(t.back)
                .on_press(back)
                .build(k),
        );
    } else {
        bar = bar.child(icon(&icons::SPARKLE, 15.0).color(k.c.text_accent));
    }
    bar = bar
        .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
        .child(spacer());
    if back.is_none() {
        bar = bar.child(
            IconButton::new(&icons::GEAR)
                .variant(IconButtonVariant::GhostMuted)
                .label(t.settings)
                .shortcut("⌘,")
                .on_press(|s: &mut State| s.open_settings())
                .build(k),
        );
        let pin = IconButton::new(&icons::PIN)
            .variant(if s.pinned {
                IconButtonVariant::Ghost
            } else {
                IconButtonVariant::GhostMuted
            })
            .label(if s.pinned { t.unpin } else { t.pin })
            .on_press(|s: &mut State| s.toggle_pin())
            .build(k);
        bar = bar.child(if s.pinned {
            pin.fill(k.c.bg_accent.alpha(0.16))
        } else {
            pin
        });
    }
    bar.child(
        IconButton::new(&icons::CLOSE)
            .variant(IconButtonVariant::GhostMuted)
            .label(t.hide)
            .shortcut("Esc")
            .on_press(|s: &mut State| s.hide())
            .build(k),
    )
}

fn actions(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    row()
        .w_full()
        // Long labels and language names don't fit one line in some languages.
        .flex_wrap()
        .center_y()
        .gap(8.0)
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
        .child(rewrite(s, k, layer))
}

/// Rewrite, with the styles to rewrite in as its menu.
fn rewrite(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let open = s.menu_open(Menu::Rewrite);
    let mut menu = Popover::new("styles")
        .trigger(buttons::menu_button(k, t.rewrite, &icons::PEN, |b| {
            b.shortcut("⌘3")
                .on_press(|s: &mut State| s.toggle_menu(Menu::Rewrite))
        }))
        .open(open)
        .placement(Placement::BottomStart)
        .label(t.rewrite)
        .padding(4.0)
        .on_close(|s: &mut State| s.menu = None);
    // A closed popover drops its content, so skip building the list.
    if open {
        menu = menu.child(col().w(200.0).gap(2.0).children(Style::ALL.map(|style| {
            row()
                .key(format!("style-{style:?}"))
                .role("menuitem")
                .label(t.style(style))
                .w_full()
                .h(30.0)
                .px(8.0)
                .center_y()
                .radius(6.0)
                .hover(k.c.hover)
                .on_press(move |s: &mut State| s.start(Action::Rewrite(style)))
                .child(text(t.style(style)).font(k.ty.ui).color(k.c.text_base))
        })));
    }
    menu.build(k, layer)
}

/// Translate and the language it writes in, joined in one control.
fn translate(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let open = s.menu_open(Menu::Language);
    let mut picker = Popover::new("languages")
        .trigger(
            Button::new(t.language(&s.settings.language))
                .icon(&icons::CHEV_DOWN)
                .variant(ButtonVariant::Ghost)
                .help(t.translation_language)
                .on_press(|s: &mut State| s.toggle_menu(Menu::Language))
                .build(k)
                .radius(14.0)
                .hover(Color::TRANSPARENT),
        )
        .open(open)
        .placement(Placement::BottomStart)
        .label(t.translation_language)
        .padding(4.0)
        .on_close(|s: &mut State| s.menu = None);
    // A closed popover drops its content, so skip building the list.
    if open {
        picker = picker.child(languages(s, k));
    }
    let picker = picker.build(k, layer);
    let group = row()
        .h(28.0)
        .px(2.0)
        .center_y()
        .child(
            Button::new(t.translate)
                .icon(&icons::GLOBE)
                .variant(ButtonVariant::Ghost)
                .shortcut("⌘2")
                .on_press(|s: &mut State| s.start(Action::Translate))
                .build(k)
                .radius(14.0)
                .hover(Color::TRANSPARENT),
        )
        .child(el().w(1.0).h(16.0).fill(k.c.border_muted))
        .child(picker);
    buttons::surface(k, Tone::Secondary, group)
}

/// The languages, favourites first; a star marks or unmarks one.
fn languages(s: &State, k: &Kit) -> El {
    let t = s.t();
    let (mut favourites, mut rest) = s.settings.languages();
    t.sort_languages(&mut favourites);
    t.sort_languages(&mut rest);
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
            Switch::new(s.show_changes)
                .label(t.show_changes)
                .on_toggle(|s: &mut State| s.show_changes = !s.show_changes)
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
        Status::Running { text: partial, .. } if partial.is_empty() => {
            text(t.thinking).font(k.ty.body).color(k.c.text_faint)
        }
        Status::Running { text: partial, .. } => plain(partial),
        Status::Done { .. } if answer.action == Action::Fix && !changed => {
            note(k, t.nothing_to_fix, NoteKind::Info)
        }
        Status::Done { changes, .. } if changed && s.show_changes => {
            scroll_box(diff_text(changes, k))
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
                    |b| b.shortcut("⇧⌘C").on_press(|s: &mut State| s.copy()),
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
fn diff_text(changes: &[Piece], k: &Kit) -> El {
    let mut content = String::new();
    let mut spans = Vec::new();
    let mut strikes = Vec::new();
    let mut previous = Kind::Same;
    for piece in changes {
        // A thin space keeps a word from running into the one that replaces it.
        if previous == Kind::Removed && piece.kind == Kind::Added {
            content.push('\u{2009}');
        }
        previous = piece.kind;
        let start = content.len();
        content.push_str(&piece.text);
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
    rich(content, spans)
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
#[derive(Clone, Copy, PartialEq, Eq)]
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
            _ if s.menu.is_some() => s.menu = None,
            Page::Settings => s.back(),
            Page::Main => s.hide(),
        }
        return true;
    }
    if key.key == Key::Enter && key.mods.cmd {
        s.replace();
        return true;
    }
    if key.is_command('1') {
        s.start(Action::Fix);
    } else if key.is_command('2') {
        s.start(Action::Translate);
    } else if key.is_command('3') {
        s.toggle_menu(Menu::Rewrite);
    } else if key.is_command('c') && key.mods.shift {
        s.copy();
    } else if key.is_command(',') {
        s.open_settings();
    } else if key.is_command('w') {
        s.hide();
    } else if key.is_command('q') {
        s.quit(tx);
    } else {
        return false;
    }
    true
}
