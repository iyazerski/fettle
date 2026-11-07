//! What the window draws: the text at the top, the actions under it, and the
//! answer at the bottom; or the settings ([`settings_view`](super::settings_view)).

use rune::{
    Color, Cx, El, Key, KeyEvent, ScrollSpec, Span, SystemMaterial, Tx, col, el, icon, rich, row,
    spacer, text,
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Kit, Spinner, Switch, TextArea,
};

use super::icons;
use super::settings_view;
use super::state::{Answer, Page, State, Status};
use crate::diff::{self, Kind};
use crate::prompts::Action;
use crate::settings::LANGUAGES;

/// The window's corner radius.
pub const CORNER: f32 = 16.0;
pub const WIDTH: f32 = 520.0;
/// The tallest the answer gets before it scrolls.
const ANSWER_MAX: f32 = 320.0;

pub fn view(s: &State, _cx: &mut Cx<State>) -> El {
    // Light, whatever the system's appearance: the window reads as paper.
    let k = rune_theme_opencode::kit(false);
    let page = match s.page {
        Page::Main => main(s, k),
        Page::Settings => settings_view::view(s, k),
    };
    // Not `rune_kit::root`: it fills the window, and the window fits the content.
    col()
        .key("window")
        .w_full()
        .radius(CORNER)
        .system_material(SystemMaterial::Popover)
        .child(
            col()
                .w_full()
                .radius(CORNER)
                .fill(Color::WHITE.alpha(0.8))
                .border(1.0, k.c.border_base.alpha(0.6))
                .padding(14.0)
                .gap(12.0)
                .child(page),
        )
}

fn main(s: &State, k: &Kit) -> El {
    let running = s
        .answer
        .as_ref()
        .is_some_and(|a| a.status == Status::Running);
    let mut page = col()
        .w_full()
        .gap(12.0)
        .child(header(k, "Quill", None))
        .child(
            TextArea::new(&s.source)
                .label("Text")
                .placeholder("Select text in any app and press the shortcut, or type here")
                .width(None)
                .auto_grow(2, 8)
                .on_edit(|s: &mut State| &mut s.source)
                .build(k),
        );
    if let Some(notice) = &s.notice {
        page = page.child(note(k, notice, false));
    }
    page = page.child(actions(s, k, running));
    if s.language_open {
        page = page.child(languages(s, k));
    }
    if let Some(answer) = &s.answer {
        page = page.child(divider(k)).child(answer_view(s, answer, k));
    }
    page
}

/// The title row, with a back button on sub-pages and the hide button.
pub fn header(k: &Kit, title: &str, back: Option<fn(&mut State)>) -> El {
    let mut bar = row().w_full().center_y().gap(6.0);
    if let Some(back) = back {
        bar = bar.child(
            IconButton::new(&icons::BACK)
                .variant(IconButtonVariant::GhostMuted)
                .label("Back")
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
                .label("Settings")
                .shortcut("⌘,")
                .on_press(|s: &mut State| s.open_settings())
                .build(k),
        );
    }
    bar.child(
        IconButton::new(&icons::CLOSE)
            .variant(IconButtonVariant::GhostMuted)
            .label("Hide")
            .shortcut("Esc")
            .on_press(|s: &mut State| s.hide())
            .build(k),
    )
}

fn actions(s: &State, k: &Kit, running: bool) -> El {
    let mut bar = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(
            Button::new("Improve")
                .icon(&icons::SPARKLE)
                .variant(ButtonVariant::Contrast)
                .shortcut("⌘1")
                .on_press(|s: &mut State| s.start(Action::Improve))
                .build(k),
        )
        .child(
            Button::new("Translate")
                .icon(&icons::GLOBE)
                .variant(ButtonVariant::Neutral)
                .shortcut("⌘2")
                .on_press(|s: &mut State| s.start(Action::Translate))
                .build(k),
        )
        .child(
            Button::new(format!("to {}", s.settings.language))
                .icon(&icons::CHEV_DOWN)
                .variant(ButtonVariant::Ghost)
                .expanded(s.language_open)
                .help("Translation language")
                .on_press(|s: &mut State| s.language_open = !s.language_open)
                .build(k),
        )
        .child(spacer());
    if running {
        bar = bar.child(
            Button::new("Stop")
                .icon(&icons::STOP)
                .variant(ButtonVariant::GhostMuted)
                .on_press(|s: &mut State| s.stop())
                .build(k),
        );
    }
    bar
}

/// The translation languages, inline: a popup would run past the window's bottom.
fn languages(s: &State, k: &Kit) -> El {
    let mut chips = row().w_full().flex_wrap().gap(6.0);
    for &language in LANGUAGES {
        let current = s.settings.language == language;
        chips = chips.child(
            Button::new(language)
                .variant(if current {
                    ButtonVariant::Contrast
                } else {
                    ButtonVariant::Outline
                })
                .on_press(move |s: &mut State| s.set_language(language))
                .build(k),
        );
    }
    chips
}

fn answer_view(s: &State, answer: &Answer, k: &Kit) -> El {
    let title = match answer.action {
        Action::Improve => "Improved".to_owned(),
        Action::Translate => format!("{} translation", s.settings.language),
    };
    let changed = diff::changed(&answer.changes);
    let mut top = row().w_full().center_y().gap(8.0).child(
        text(title)
            .font(k.ty.text_small_medium)
            .color(k.c.text_muted),
    );
    if answer.status == Status::Running {
        top = top.child(Spinner::new().size(12.0).build(k));
    }
    top = top.child(spacer());
    if answer.status == Status::Done && answer.action == Action::Improve && changed {
        top = top.child(
            Switch::new(s.show_changes)
                .label("Show changes")
                .on_toggle(|s: &mut State| s.show_changes = !s.show_changes)
                .build(k),
        );
    }
    let body = match &answer.status {
        Status::Failed(e) => note(k, e, true),
        Status::Done if answer.action == Action::Improve && !changed => {
            note(k, "Looks good. Nothing to fix.", false)
        }
        Status::Done if answer.action == Action::Improve && s.show_changes => {
            scroll_box(k, diff_text(answer, k))
        }
        _ if answer.text.is_empty() => text("Thinking…").font(k.ty.body).color(k.c.text_faint),
        _ => scroll_box(
            k,
            text(answer.text.clone())
                .font(k.ty.body)
                .color(k.c.text_base)
                .wrap()
                .selectable(true),
        ),
    };
    let mut section = col().w_full().gap(8.0).child(top).child(body);
    if answer.status == Status::Done {
        section = section.child(
            row()
                .w_full()
                .gap(8.0)
                .child(spacer())
                .child(
                    Button::new(if answer.copied { "Copied" } else { "Copy" })
                        .icon(if answer.copied {
                            &icons::CHECK
                        } else {
                            &icons::COPY
                        })
                        .variant(ButtonVariant::Neutral)
                        .shortcut("⇧⌘C")
                        .on_press(|s: &mut State| s.copy())
                        .build(k),
                )
                .child(
                    Button::new("Replace")
                        .icon(&icons::REPLACE)
                        .variant(ButtonVariant::Submit)
                        .shortcut("⌘↩")
                        .help("Paste over the selected text")
                        .on_press(|s: &mut State| s.replace())
                        .build(k),
                ),
        );
    }
    section
}

/// The improvement with removed words struck in red and added ones in green.
fn diff_text(answer: &Answer, k: &Kit) -> El {
    let mut content = String::new();
    let mut spans = Vec::new();
    let mut previous = Kind::Same;
    for piece in &answer.changes {
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
            Kind::Removed => spans.push(Span {
                color: Some(k.c.danger.fg),
                bg: Some(k.c.danger.bg),
                bg_pad: 1.5,
                underline: Some(k.c.danger.icon),
                ..span
            }),
            Kind::Added => spans.push(Span {
                color: Some(k.c.success.fg),
                bg: Some(k.c.success.bg),
                bg_pad: 1.5,
                weight: Some(560.0),
                ..span
            }),
        }
    }
    rich(content, spans)
        .font(k.ty.body)
        .color(k.c.text_base)
        .selectable(true)
}

/// `content` in a well that scrolls past [`ANSWER_MAX`].
fn scroll_box(k: &Kit, content: El) -> El {
    col()
        .w_full()
        .max_h(ANSWER_MAX)
        .padding(10.0)
        .radius(k.radius.md)
        .fill(k.c.bg_base.alpha(0.7))
        .border(1.0, k.c.border_muted)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}

pub fn note(k: &Kit, message: &str, failure: bool) -> El {
    let color = if failure {
        k.c.danger.fg
    } else {
        k.c.text_muted
    };
    let mut line = row().w_full().gap(6.0);
    if failure {
        line = line.child(icon(&icons::ALERT, 14.0).color(k.c.danger.icon));
    }
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
            _ if s.language_open => s.language_open = false,
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
        s.start(Action::Improve);
    } else if key.is_command('2') {
        s.start(Action::Translate);
    } else if key.is_command('c') && key.mods.shift {
        s.copy();
    } else if key.is_command(',') {
        s.open_settings();
    } else if key.is_command('w') {
        s.hide();
    } else if key.is_command('q') {
        tx.quit();
    } else {
        return false;
    }
    true
}
