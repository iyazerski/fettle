//! The settings page: where the model runs, which model, how hard it thinks,
//! and the shortcut, login item and permission.

use rune::{El, ScrollSpec, col, icon, row, spacer, text};
use rune_kit::{
    Button, ButtonVariant, Kit, SegmentedControl, ShortcutRecorder, Switch, TextInput,
    shortcut_error_message,
};

use super::icons;
use super::state::{Login, Models, State};
use super::view::{divider, header, note};
use crate::settings::{Provider, Reasoning};

/// The tallest the model list gets before it scrolls.
const MODELS_MAX: f32 = 150.0;

pub fn view(s: &State, k: &Kit) -> El {
    let mut page = col()
        .w_full()
        .gap(12.0)
        .child(header(k, "Settings", Some(State::back)));
    if let Some(notice) = &s.notice {
        page = page.child(note(k, notice, true));
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
        .child(model(s, k))
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
        .child(
            row()
                .w_full()
                .center_y()
                .child(
                    text(format!("Quill {}", crate::VERSION))
                        .font(k.ty.text_small)
                        .color(k.c.text_faint),
                )
                .child(spacer())
                .child(
                    Button::new("Quit Quill")
                        .variant(ButtonVariant::GhostMuted)
                        .shortcut("⌘Q")
                        .on_press_tx(|_: &mut State, tx| tx.quit())
                        .build(k),
                ),
        )
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
            let mut block = col().w_full().gap(6.0).child(
                Button::new("Sign in with ChatGPT")
                    .variant(ButtonVariant::Contrast)
                    .on_press(|s: &mut State| s.sign_in())
                    .build(k),
            );
            if let Login::Failed(e) = login {
                block = block.child(note(k, e, true));
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
                .on_change(|s: &mut State, _| s.form_edited())
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
                .on_change(|s: &mut State, _| s.form_edited())
                .build(k),
        ))
}

fn model(s: &State, k: &Kit) -> El {
    let loading = matches!(s.form.models, Models::Loading);
    let field = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            TextInput::new(&s.form.model)
                .label("Model")
                .placeholder("Model ID")
                .width(None)
                .on_edit(|s: &mut State| &mut s.form.model)
                .on_change(|s: &mut State, _| s.form_edited())
                .build(k)
                .grow(1.0),
        )
        .child(
            Button::new(if loading { "Loading…" } else { "Show models" })
                .variant(ButtonVariant::Neutral)
                .disabled(loading)
                .on_press(|s: &mut State| s.load_models())
                .build(k),
        );
    let mut block = col().w_full().gap(6.0).child(labeled(k, "Model", field));
    match &s.form.models {
        Models::Ready(models) => {
            let mut chips = row().w_full().flex_wrap().gap(6.0);
            for model in models {
                let current = s.settings.model() == model;
                let pick = model.clone();
                chips = chips.child(
                    Button::new(model.clone())
                        .variant(if current {
                            ButtonVariant::Contrast
                        } else {
                            ButtonVariant::Outline
                        })
                        .on_press(move |s: &mut State| s.pick_model(&pick))
                        .build(k),
                );
            }
            block = block.child(
                col()
                    .w_full()
                    .max_h(MODELS_MAX)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(chips),
            );
        }
        Models::Failed(e) => block = block.child(note(k, e, true)),
        Models::Idle | Models::Loading => {}
    }
    block
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
        .child(
            Button::new("Allow…")
                .variant(ButtonVariant::Contrast)
                .help("Quill copies the selection and pastes the result with ⌘C and ⌘V")
                .on_press(|s: &mut State| s.ask_accessibility())
                .build(k),
        )
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
                .w(110.0),
        )
        .child(col().grow(1.0).child(control))
}
