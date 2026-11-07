//! The settings page: where the model runs, which model, how hard it thinks,
//! the look, the shortcut, login item and permission, updates, and credits.

use rune::{Cursor, El, col, icon, row, spacer, text};
use rune_kit::{
    Button, ButtonVariant, Kit, Placement, SegmentedControl, Select, SelectAppearance,
    ShortcutRecorder, Spinner, Switch, TextInput, shortcut_error_message,
};

use super::buttons::{self, Tone};
use super::icons;
use super::settings_state::{Login, Models, Update};
use super::state::{Menu, State};
use super::view::{NoteKind, WIDTH, divider, header, note};
use crate::mac;
use crate::settings::{Appearance, Missing, Provider, Reasoning};

const AUTHOR: &str = "Ihar Yazerski";
const AUTHOR_URL: &str = "https://github.com/iyazerski";
const REPO_URL: &str = "https://github.com/iyazerski/quill";
const LICENSE_URL: &str = "https://github.com/iyazerski/quill/blob/main/LICENSE";
const NOTICES_URL: &str = "https://github.com/iyazerski/quill/blob/main/THIRD_PARTY_LICENSES.md";

/// The label column's width.
const LABEL: f32 = 110.0;
/// A control's width beside its label: the window less its padding, the label and the gap.
const CONTROL: f32 = WIDTH - 2.0 * 14.0 - LABEL - 12.0;

pub fn view(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut page = col()
        .w_full()
        .gap(12.0)
        .child(header(k, "Settings", Some(State::back)));
    if let Some(error) = &s.form.error {
        page = page.child(note(k, &error.to_string(), NoteKind::Failure));
    }
    page.child(section(k, "Model"))
        .child(
            SegmentedControl::new(
                &["ChatGPT plan", "OpenAI-compatible API"],
                Some(usize::from(s.settings.provider == Provider::Api)),
            )
            .width(None)
            .on_pick(|s: &mut State, i, _| {
                s.set_provider(if i == 0 {
                    Provider::ChatGpt
                } else {
                    Provider::Api
                })
            })
            .build(k),
        )
        .child(match s.settings.provider {
            Provider::ChatGpt => chatgpt(s, k),
            Provider::Api => api(s, k),
        })
        .children(model(s, k, layer))
        .child(labeled(
            k,
            "Reasoning",
            SegmentedControl::new(
                &Reasoning::ALL.map(Reasoning::label),
                Reasoning::ALL
                    .iter()
                    .position(|r| *r == s.settings.reasoning),
            )
            .width(None)
            .on_pick(|s: &mut State, i, _| s.set_reasoning(Reasoning::ALL[i]))
            .build(k),
        ))
        .child(divider(k))
        .child(section(k, "General"))
        .child(labeled(
            k,
            "Appearance",
            SegmentedControl::new(
                &Appearance::ALL.map(Appearance::label),
                Appearance::ALL
                    .iter()
                    .position(|a| *a == s.settings.appearance),
            )
            .width(None)
            .on_pick(|s: &mut State, i, _| s.set_appearance(Appearance::ALL[i]))
            .build(k),
        ))
        .child(labeled(
            k,
            "Shortcut",
            ShortcutRecorder::new("shortcut", &s.form.shortcut, |s: &mut State| {
                &mut s.form.shortcut
            })
            .label("Open Quill")
            .on_change(|s: &mut State, shortcut, _| s.change_shortcut(shortcut))
            .error_message(shortcut_error_message)
            .width(200.0)
            .build(k),
        ))
        .child(labeled(
            k,
            "Start at login",
            Switch::new(s.form.at_login)
                .hide_label()
                .label("Start at login")
                .on_toggle(|s: &mut State| s.toggle_at_login())
                .build(k),
        ))
        .child(labeled(k, "Accessibility", accessibility(s, k)))
        .child(divider(k))
        .child(footer(s, k))
        .child(credits(k))
}

/// Who made Quill, its license, the licenses of what it's built from, and its
/// source, each a link.
fn credits(k: &Kit) -> El {
    let faint = |label: &str| text(label).font(k.ty.text_small).color(k.c.text_faint);
    let link = |label: &str, url: &'static str| {
        text(label)
            .font(k.ty.text_small)
            .color(k.c.text_muted)
            .cursor(Cursor::Pointer)
            .on_press(move |_: &mut State| mac::open_url(url))
    };
    row()
        .w_full()
        .center_y()
        .gap(5.0)
        .child(faint("Made by"))
        .child(link(AUTHOR, AUTHOR_URL))
        .child(faint("·"))
        .child(link("MIT License", LICENSE_URL))
        .child(faint("·"))
        .child(link("Open-source licenses", NOTICES_URL))
        .child(faint("·"))
        .child(link("GitHub", REPO_URL))
}

/// The version, the update button, and Quit.
fn footer(s: &State, k: &Kit) -> El {
    let mut bar = row().w_full().center_y().gap(8.0).child(
        text(format!("Quill {}", crate::VERSION))
            .font(k.ty.text_small)
            .color(k.c.text_faint),
    );
    let status = |label: String| text(label).font(k.ty.text_small).color(k.c.text_muted);
    let check = || {
        Button::new("Check for updates")
            .variant(ButtonVariant::Ghost)
            .on_press(|s: &mut State| s.check_update())
            .build(k)
    };
    bar = match &s.update {
        Update::Idle | Update::Failed(_) => bar.child(check()),
        Update::Checking => bar
            .child(Spinner::new().size(12.0).build(k))
            .child(status("Checking…".into())),
        Update::Latest => bar
            .child(icon(&icons::CHECK, 14.0).color(k.c.success.icon))
            .child(status("Up to date".into())),
        Update::Available(version) => bar.child(buttons::button(
            k,
            Tone::Primary,
            &format!("Update to {version}"),
            Some(&icons::DOWNLOAD),
            |b| {
                b.help("Download it, then restart Quill")
                    .on_press(|s: &mut State| s.install_update())
            },
        )),
        Update::Installing(version) => bar
            .child(Spinner::new().size(12.0).build(k))
            .child(status(format!("Updating to {version}…"))),
    };
    bar = bar.child(spacer()).child(
        Button::new("Quit Quill")
            .variant(ButtonVariant::GhostMuted)
            .shortcut("⌘Q")
            .on_press_tx(|s: &mut State, tx| s.quit(tx))
            .build(k),
    );
    match &s.update {
        Update::Failed(e) => col()
            .w_full()
            .gap(6.0)
            .child(note(k, e, NoteKind::Failure))
            .child(bar),
        _ => bar,
    }
}

fn chatgpt(s: &State, k: &Kit) -> El {
    let line = row().w_full().center_y().gap(8.0);
    match (&s.settings.chatgpt, &s.form.login) {
        (_, Login::Waiting(_)) => line
            .child(
                text("Finish signing in in your browser…")
                    .font(k.ty.ui)
                    .color(k.c.text_muted),
            )
            .child(spacer())
            .child(
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut State| s.cancel_sign_in())
                    .build(k),
            ),
        (Some(auth), _) => line
            .child(icon(&icons::CHECK, 14.0).color(k.c.success.icon))
            .child(
                text(format!(
                    "Signed in as {}",
                    auth.email.as_deref().unwrap_or("a ChatGPT user")
                ))
                .font(k.ty.ui)
                .color(k.c.text_base)
                .truncate()
                .grow(1.0),
            )
            .child(
                Button::new("Sign out")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut State| s.sign_out())
                    .build(k),
            ),
        (None, login) => {
            let mut block = col().w_full().gap(6.0).child(buttons::button(
                k,
                Tone::Primary,
                "Sign in with ChatGPT",
                None,
                |b| b.on_press(|s: &mut State| s.sign_in()),
            ));
            if let Login::Failed(e) = login {
                block = block.child(note(k, e, NoteKind::Failure));
            }
            block
        }
    }
}

fn api(s: &State, k: &Kit) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(labeled(
            k,
            "API URL",
            TextInput::new(&s.form.api_base)
                .label("API URL")
                .placeholder("https://api.openai.com/v1")
                .width(None)
                .on_edit(|s: &mut State| &mut s.form.api_base)
                .on_change(|s: &mut State, _| s.api_edited())
                .build(k),
        ))
        .child(labeled(
            k,
            "API key",
            TextInput::new(&s.form.api_key)
                .secure()
                .label("API key")
                .placeholder("sk-…, or empty for none")
                .width(None)
                .on_edit(|s: &mut State| &mut s.form.api_key)
                .on_change(|s: &mut State, _| s.api_edited())
                .build(k),
        ))
}

/// The model dropdown, once the provider can list its models.
fn model(s: &State, k: &Kit, layer: &mut Vec<El>) -> Option<El> {
    if matches!(
        s.settings.missing(),
        Some(Missing::SignIn | Missing::ApiUrl)
    ) {
        return None;
    }
    let loading = matches!(s.form.models, Models::Loading(_));
    let models: &[String] = match &s.form.models {
        Models::Ready(models) => models,
        _ => &[],
    };
    let current = Some(s.settings.model()).filter(|m| !m.is_empty());
    let select = Select::new("model", models)
        .label("Model")
        .appearance(SelectAppearance::Field)
        .width(CONTROL)
        .current(current)
        .placeholder(if loading {
            "Loading models…"
        } else {
            "Choose a model"
        })
        .placement(Placement::BottomStart)
        .open(s.menu_open(Menu::Model) && !models.is_empty())
        .on_toggle(|s: &mut State| s.toggle_models())
        .on_close(|s: &mut State| s.menu = None)
        .on_choose(|s: &mut State, model, _| s.pick_model(model))
        .build(k, layer);
    let mut block = col().w_full().gap(6.0).child(labeled(k, "Model", select));
    if let Models::Failed(e) = &s.form.models {
        block = block.child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(note(k, e, NoteKind::Failure).grow(1.0))
                .child(
                    Button::new("Try again")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut State| s.load_models())
                        .build(k),
                ),
        );
    }
    Some(block)
}

fn accessibility(s: &State, k: &Kit) -> El {
    if s.trusted {
        return row()
            .gap(6.0)
            .center_y()
            .child(icon(&icons::CHECK, 14.0).color(k.c.success.icon))
            .child(text("Allowed").font(k.ty.ui).color(k.c.text_base));
    }
    row()
        .gap(8.0)
        .center_y()
        .child(buttons::button(k, Tone::Primary, "Allow…", None, |b| {
            b.help("Quill copies the selection and pastes the result with ⌘C and ⌘V")
                .on_press(|s: &mut State| s.ask_accessibility())
        }))
        .child(
            Button::new("Check again")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut State| s.recheck_accessibility())
                .build(k),
        )
}

fn section(k: &Kit, title: &str) -> El {
    text(title.to_owned())
        .font(k.ty.text_small_medium)
        .color(k.c.text_muted)
}

/// `control` with `label` on its left, in a fixed column so the controls line up.
fn labeled(k: &Kit, label: &str, control: El) -> El {
    row()
        .w_full()
        .center_y()
        .gap(12.0)
        .child(
            text(label.to_owned())
                .font(k.ty.ui)
                .color(k.c.text_base)
                .w(LABEL),
        )
        .child(col().grow(1.0).child(control))
}
