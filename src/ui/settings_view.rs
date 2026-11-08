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
use crate::i18n::{Locale, fill};
use crate::llm::on_device::Availability;
use crate::mac;
use crate::settings::{Appearance, Missing, Provider, Reasoning};

/// The providers, in the order the settings list them.
const PROVIDERS: [Provider; 3] = [Provider::OnDevice, Provider::ChatGpt, Provider::Api];

const AUTHOR: &str = "Ihar Yazerski";
const AUTHOR_URL: &str = "https://buymeacoffee.com/iyazerski";
const REPO_URL: &str = "https://github.com/iyazerski/fettle";
const LICENSE_URL: &str = "https://github.com/iyazerski/fettle/blob/main/LICENSE";
const NOTICES_URL: &str = "https://github.com/iyazerski/fettle/blob/main/THIRD_PARTY_LICENSES.md";

/// The label column's width.
const LABEL: f32 = 110.0;
/// A control's width beside its label: the window less its padding, the label and the gap.
const CONTROL: f32 = WIDTH - 2.0 * 14.0 - LABEL - 12.0;

pub fn view(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let sk = segmented_kit(k);
    let mut page = col()
        .w_full()
        .gap(12.0)
        .child(header(s, k, t.settings, Some(State::back)));
    if let Some(error) = &s.form.error {
        page = page.child(note(k, error.message(t), NoteKind::Failure));
    }
    page.child(section(k, t.model))
        .child(
            SegmentedControl::new(
                // The full "OpenAI-compatible API" doesn't fit a third of the row.
                &[t.on_device, t.chatgpt_plan, "API"],
                PROVIDERS.iter().position(|p| *p == s.settings.provider),
            )
            .width(None)
            .on_pick(|s: &mut State, i, _| s.set_provider(PROVIDERS[i]))
            .build(&sk),
        )
        .child(match s.settings.provider {
            Provider::OnDevice => on_device(s, k),
            Provider::ChatGpt => chatgpt(s, k),
            Provider::Api => api(s, k),
        })
        .children(model(s, k, layer))
        // Apple's model has no reasoning effort to set.
        .children((s.settings.provider != Provider::OnDevice).then(|| {
            labeled(
                k,
                t.reasoning,
                SegmentedControl::new(
                    &t.reasoning_levels,
                    Reasoning::ALL
                        .iter()
                        .position(|r| *r == s.settings.reasoning),
                )
                .width(None)
                .on_pick(|s: &mut State, i, _| s.set_reasoning(Reasoning::ALL[i]))
                .build(&sk),
            )
        }))
        .child(divider(k))
        .child(section(k, t.general))
        .child(labeled(
            k,
            t.appearance,
            SegmentedControl::new(
                &t.appearances,
                Appearance::ALL
                    .iter()
                    .position(|a| *a == s.settings.appearance),
            )
            .width(None)
            .on_pick(|s: &mut State, i, _| s.set_appearance(Appearance::ALL[i]))
            .build(&sk),
        ))
        .child(labeled(k, t.app_language, app_language(s, k, layer)))
        .child(labeled(
            k,
            t.shortcut,
            ShortcutRecorder::new("shortcut", &s.form.shortcut, |s: &mut State| {
                &mut s.form.shortcut
            })
            .label(t.open_fettle)
            .on_change(|s: &mut State, shortcut, _| s.change_shortcut(shortcut))
            .error_message(shortcut_error_message)
            .width(200.0)
            .build(k),
        ))
        .child(labeled(
            k,
            t.start_at_login,
            Switch::new(s.form.at_login)
                .hide_label()
                .label(t.start_at_login)
                .on_toggle(|s: &mut State| s.toggle_at_login())
                .build(k),
        ))
        .child(labeled(k, t.accessibility, accessibility(s, k)))
        .child(divider(k))
        .child(footer(s, k))
        .child(credits(s, k))
}

/// The window's language: System, then each language in its own name.
fn app_language(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let options: Vec<&str> = std::iter::once(t.system_language)
        .chain(Locale::ALL.map(Locale::native_name))
        .collect();
    let current = s
        .settings
        .app_language
        .map_or(t.system_language, Locale::native_name);
    Select::new("app-language", &options)
        .label(t.app_language)
        .appearance(SelectAppearance::Field)
        .width(200.0)
        .current(Some(current))
        .placement(Placement::BottomStart)
        .open(s.menu_open(Menu::AppLanguage))
        .on_toggle(|s: &mut State| s.toggle_menu(Menu::AppLanguage))
        .on_close(|s: &mut State| s.menu = None)
        .on_choose(|s: &mut State, name, _| s.pick_app_language(name))
        .build(k, layer)
}

/// Return the kit for segmented controls. In light mode, their track is grey so the
/// white selected segment stands out from the rest.
fn segmented_kit(k: &Kit) -> Kit {
    let mut sk = k.clone();
    if !k.dark {
        // The default track is nearly as white as the selected segment on the light glass.
        sk.c.layer_01 = k.dark_a(0.07);
    }
    sk
}

/// Who made Fettle, its license, the licenses of what it's built from, and its
/// source, each a link.
fn credits(s: &State, k: &Kit) -> El {
    let t = s.t();
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
        .child(faint(t.made_by))
        .child(link(AUTHOR, AUTHOR_URL).help(t.buy_coffee))
        .child(faint("·"))
        .child(link(t.mit_license, LICENSE_URL))
        .child(faint("·"))
        .child(link(t.notices, NOTICES_URL))
        .child(faint("·"))
        .child(link("GitHub", REPO_URL))
}

/// The version, the update button, and Quit.
fn footer(s: &State, k: &Kit) -> El {
    let t = s.t();
    let mut bar = row().w_full().center_y().gap(8.0).child(
        text(format!("Fettle {}", crate::VERSION))
            .font(k.ty.text_small)
            .color(k.c.text_faint),
    );
    let status = |label: String| text(label).font(k.ty.text_small).color(k.c.text_muted);
    let check = || {
        Button::new(t.check_updates)
            .variant(ButtonVariant::Ghost)
            .on_press(|s: &mut State| s.check_update())
            .build(k)
    };
    bar = match &s.update {
        Update::Idle | Update::Failed(_) => bar.child(check()),
        Update::Checking => bar
            .child(Spinner::new().size(12.0).build(k))
            .child(status(t.checking.into())),
        Update::Latest => bar
            .child(icon(&icons::CHECK, 14.0).color(k.c.success.icon))
            .child(status(t.up_to_date.into())),
        Update::Available(version) => bar.child(buttons::button(
            k,
            Tone::Primary,
            &fill(t.update_to, version),
            Some(&icons::DOWNLOAD),
            |b| {
                b.help(t.update_help)
                    .on_press(|s: &mut State| s.install_update())
            },
        )),
        Update::Installing(version) => bar
            .child(Spinner::new().size(12.0).build(k))
            .child(status(fill(t.updating, version))),
    };
    bar = bar.child(spacer()).child(
        Button::new(t.quit)
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

/// Whether Apple's on-device model can run, with a way to System Settings when
/// a setting there stops it.
pub(super) fn on_device(s: &State, k: &Kit) -> El {
    let t = s.t();
    if s.on_device == Availability::Ready {
        return row()
            .w_full()
            .center_y()
            .gap(6.0)
            .child(icon(&icons::CHECK, 14.0).color(k.c.success.icon))
            .child(
                text(t.on_device_ready)
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .wrap()
                    .grow(1.0),
            );
    }
    // A setting in System Settings can fix these, so they read as a step, not an error.
    let fixable = matches!(
        s.on_device,
        Availability::TurnedOff | Availability::NotReady
    );
    let kind = if fixable {
        NoteKind::Info
    } else {
        NoteKind::Failure
    };
    let mut line = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(note(k, t.on_device_status(s.on_device), kind).grow(1.0));
    if fixable {
        line = line.child(
            Button::new(t.open_system_settings)
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut State| s.open_apple_intelligence())
                .build(k),
        );
    }
    line
}

pub(super) fn chatgpt(s: &State, k: &Kit) -> El {
    let t = s.t();
    let line = row().w_full().center_y().gap(8.0);
    match (&s.settings.chatgpt, &s.form.login) {
        (_, Login::Waiting(_)) => line
            .child(text(t.finish_sign_in).font(k.ty.ui).color(k.c.text_muted))
            .child(spacer())
            .child(
                Button::new(t.cancel)
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut State| s.cancel_sign_in())
                    .build(k),
            ),
        (Some(auth), _) => line
            .child(icon(&icons::CHECK, 14.0).color(k.c.success.icon))
            .child(
                text(fill(
                    t.signed_in_as,
                    auth.email.as_deref().unwrap_or(t.chatgpt_user),
                ))
                .font(k.ty.ui)
                .color(k.c.text_base)
                .truncate()
                .grow(1.0),
            )
            .child(
                Button::new(t.sign_out)
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut State| s.sign_out())
                    .build(k),
            ),
        (None, login) => {
            let mut block = col().w_full().gap(6.0).child(buttons::button(
                k,
                Tone::Primary,
                t.sign_in,
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
    let t = s.t();
    col()
        .w_full()
        .gap(8.0)
        .child(labeled(
            k,
            t.api_url,
            TextInput::new(&s.form.api_base)
                .label(t.api_url)
                .placeholder("https://api.openai.com/v1")
                .width(None)
                .on_edit(|s: &mut State| &mut s.form.api_base)
                .on_change(|s: &mut State, _| s.api_edited())
                .build(k),
        ))
        .child(labeled(
            k,
            t.api_key,
            TextInput::new(&s.form.api_key)
                .secure()
                .label(t.api_key)
                .placeholder(t.api_key_placeholder)
                .width(None)
                .on_edit(|s: &mut State| &mut s.form.api_key)
                .on_change(|s: &mut State, _| s.api_edited())
                .build(k),
        ))
}

/// The model dropdown, once the provider can list its models.
fn model(s: &State, k: &Kit, layer: &mut Vec<El>) -> Option<El> {
    let t = s.t();
    if s.settings.provider == Provider::OnDevice
        || matches!(
            s.settings.missing(),
            Some(Missing::SignIn | Missing::ApiUrl)
        )
    {
        return None;
    }
    let loading = matches!(s.form.models, Models::Loading(_));
    let models: &[String] = match &s.form.models {
        Models::Ready(models) => models,
        _ => &[],
    };
    let current = Some(s.settings.model()).filter(|m| !m.is_empty());
    let select = Select::new("model", models)
        .label(t.model)
        .appearance(SelectAppearance::Field)
        .width(CONTROL)
        .current(current)
        .placeholder(if loading {
            t.loading_models
        } else {
            t.choose_model
        })
        .placement(Placement::BottomStart)
        .open(s.menu_open(Menu::Model) && !models.is_empty())
        .on_toggle(|s: &mut State| s.toggle_models())
        .on_close(|s: &mut State| s.menu = None)
        .on_choose(|s: &mut State, model, _| s.pick_model(model))
        .build(k, layer);
    let mut block = col().w_full().gap(6.0).child(labeled(k, t.model, select));
    if let Models::Failed(e) = &s.form.models {
        block = block.child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(note(k, e, NoteKind::Failure).grow(1.0))
                .child(
                    Button::new(t.try_again)
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut State| s.load_models())
                        .build(k),
                ),
        );
    }
    Some(block)
}

pub(super) fn accessibility(s: &State, k: &Kit) -> El {
    let t = s.t();
    if s.trusted {
        return row()
            .gap(6.0)
            .center_y()
            .child(icon(&icons::CHECK, 14.0).color(k.c.success.icon))
            .child(text(t.allowed).font(k.ty.ui).color(k.c.text_base));
    }
    row()
        .gap(8.0)
        .center_y()
        .child(buttons::button(k, Tone::Primary, t.allow, None, |b| {
            b.help(t.allow_help)
                .on_press(|s: &mut State| s.ask_accessibility())
        }))
        .child(
            Button::new(t.check_again)
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
                // Some languages need two lines for a label.
                .wrap()
                .w(LABEL),
        )
        .child(col().grow(1.0).child(control))
}
