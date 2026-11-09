//! The settings page: where the model runs, which model, whether it reasons,
//! the look, the shortcut, login item and permission, updates, and credits.
//!
//! Settings sit in rounded groups. A wide control (picker, dropdown, field)
//! fills the same column on the right of its label, and a compact one (switch,
//! status) sits at the right edge, so every row lines up.

use std::sync::OnceLock;

use rune::{Cursor, El, col, icon, row, spacer, text};
use rune_kit::{
    Button, ButtonVariant, Kit, Placement, SegmentedControl, Select, SelectAppearance,
    ShortcutRecorder, Spinner, Switch, TextInput, shortcut_error_message,
};

use super::buttons::{self, Tone};
use super::icons;
use super::settings_state::{Login, Models, Update};
use super::state::{Menu, State};
use super::view::{Lead, NoteKind, divider, header, note};
use crate::i18n::{Locale, fill};
use crate::llm::Backend;
use crate::llm::on_device::Availability;
use crate::mac;
use crate::settings::{Appearance, Provider};

/// The providers, in the order the settings list them.
const PROVIDERS: [Provider; 3] = [Provider::OnDevice, Provider::ChatGpt, Provider::Api];

const AUTHOR: &str = "Ihar Yazerski";
const AUTHOR_URL: &str = "https://buymeacoffee.com/iyazerski";
const REPO_URL: &str = "https://github.com/iyazerski/fettle";
const LICENSE_URL: &str = "https://github.com/iyazerski/fettle/blob/main/LICENSE";
const NOTICES_URL: &str = "https://github.com/iyazerski/fettle/blob/main/THIRD_PARTY_LICENSES.md";

/// The space inside a group, around its rows.
const PAD: f32 = 12.0;
/// The label column's width.
const LABEL: f32 = 130.0;
/// A wide control's width: the window less its padding, the group's, the label and the gap.
fn control(s: &State) -> f32 {
    s.width - 2.0 * 14.0 - 2.0 * PAD - LABEL - 12.0
}

pub fn view(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = s.t();
    let sk = segmented_kit(k);
    let mut page = col().w_full().gap(8.0).child(header(
        s,
        k,
        Lead::Back(State::back),
        t.settings,
        Vec::new(),
    ));
    if let Some(error) = &s.form.error {
        page = page.child(note(k, error.message(t), NoteKind::Failure));
    }

    let mut model_rows = vec![provider(s, k, sk)];
    if s.settings.provider == Provider::Api {
        model_rows.extend(api(s, k));
    }
    model_rows.extend(model(s, k, layer));
    // Apple's model has no reasoning to turn on.
    if s.settings.provider != Provider::OnDevice {
        model_rows.push(compact(
            k,
            t.reasoning,
            Some(t.reasoning_hint),
            Switch::new(s.settings.reasoning_enabled)
                .hide_label()
                .label(t.reasoning)
                .on_toggle(|s: &mut State| s.toggle_reasoning())
                .build(k),
        ));
    }

    let general_rows = vec![
        wide(
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
            .build(sk),
        ),
        wide(k, t.app_language, app_language(s, k, layer)),
        wide(
            k,
            t.shortcut,
            ShortcutRecorder::new("shortcut", &s.form.shortcut, |s: &mut State| {
                &mut s.form.shortcut
            })
            .label(t.open_fettle)
            .on_change(|s: &mut State, shortcut, _| s.change_shortcut(shortcut))
            .error_message(shortcut_error_message)
            .width(control(s))
            .build(field_kit(k)),
        ),
        compact(
            k,
            t.start_at_login,
            None,
            Switch::new(s.form.at_login)
                .hide_label()
                .label(t.start_at_login)
                .on_toggle(|s: &mut State| s.toggle_at_login())
                .build(k),
        ),
        compact(
            k,
            t.menu_bar,
            None,
            Switch::new(s.settings.menu_bar)
                .hide_label()
                .label(t.menu_bar)
                .on_toggle(|s: &mut State| s.toggle_menu_bar())
                .build(k),
        ),
        compact(
            k,
            t.underline,
            Some(t.underline_hint),
            Switch::new(s.settings.underline_mistakes)
                .hide_label()
                .label(t.underline)
                .on_toggle(|s: &mut State| s.toggle_underlines())
                .build(k),
        ),
        compact(k, t.accessibility, None, accessibility(s, k)),
    ];

    page.child(section(k, t.model))
        .child(group(k, model_rows))
        .child(section(k, t.general))
        .child(group(k, general_rows))
        .child(col().w_full().pt(8.0).child(group(k, vec![about(s, k)])))
        .child(credits(s, k))
}

/// The provider picker, with whether the picked one is ready under it.
fn provider(s: &State, k: &Kit, sk: &Kit) -> El {
    let t = s.t();
    let status = match s.settings.provider {
        Provider::OnDevice => Some(on_device(s, k)),
        Provider::ChatGpt => Some(chatgpt(s, k)),
        // Its URL and key follow as rows of their own.
        Provider::Api => None,
    };
    col()
        .w_full()
        .padding(PAD)
        .gap(10.0)
        .child(
            SegmentedControl::new(
                // The full "OpenAI-compatible API" doesn't fit a third of the row.
                &[t.on_device, t.chatgpt_plan, "API"],
                PROVIDERS.iter().position(|p| *p == s.settings.provider),
            )
            .width(None)
            .on_pick(|s: &mut State, i, _| s.set_provider(PROVIDERS[i]))
            .build(sk),
        )
        .children(status)
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
        .width(control(s))
        .current(Some(current))
        .placement(Placement::BottomStart)
        .open(s.menu_open(Menu::AppLanguage))
        .on_toggle(|s: &mut State| s.toggle_menu(Menu::AppLanguage))
        .on_close(|s: &mut State| s.close_menu())
        .on_choose(|s: &mut State, name, _| s.pick_app_language(name))
        .build(field_kit(k), layer)
}

/// Return the kit for segmented controls, made once per appearance from `k`,
/// the window's kit, with a recessed track and a raised selected segment.
fn segmented_kit(k: &Kit) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if k.dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| {
        let mut sk = k.clone();
        if k.dark {
            // By default the track is barely darker than its group and the
            // selected segment is near black, so neither reads as a control.
            sk.c.layer_01 = k.dark_a(0.22);
            sk.c.bg_base = k.light(0.14);
            sk.c.border_base = k.light(0.12);
            sk.c.border_strong = k.light(0.18);
        } else {
            // The default track is nearly as white as the selected segment on the light glass.
            sk.c.layer_01 = k.dark_a(0.07);
        }
        sk
    })
}

/// Return the kit for fields and dropdowns, made once per appearance from
/// `k`, the window's kit. In dark mode they're a shade lighter than their
/// group, not near black.
fn field_kit(k: &Kit) -> &'static Kit {
    static DARK: OnceLock<Kit> = OnceLock::new();
    static LIGHT: OnceLock<Kit> = OnceLock::new();
    let cell = if k.dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| {
        let mut fk = k.clone();
        if k.dark {
            fk.c.bg_base = k.light(0.06);
        }
        fk
    })
}

/// Who made Fettle, its license, the licenses of what it's built from, and its
/// source, each a link, centred under the groups.
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
        .pt(4.0)
        .child(spacer())
        .child(faint(t.made_by))
        .child(link(AUTHOR, AUTHOR_URL).help(t.buy_coffee))
        .child(faint("·"))
        .child(link(t.mit_license, LICENSE_URL))
        .child(faint("·"))
        .child(link(t.notices, NOTICES_URL))
        .child(faint("·"))
        .child(link("GitHub", REPO_URL))
        .child(spacer())
}

/// The version and where updating stands, with the update and Quit buttons.
fn about(s: &State, k: &Kit) -> El {
    let t = s.t();
    let status = |label: String| text(label).font(k.ty.text_small).color(k.c.text_muted);
    let state = match &s.update {
        Update::Idle | Update::Failed(_) | Update::Available(_) => None,
        Update::Checking => Some(
            row()
                .gap(6.0)
                .center_y()
                .child(Spinner::new().size(11.0).build(k))
                .child(status(t.checking.into())),
        ),
        Update::Latest => Some(
            row()
                .gap(4.0)
                .center_y()
                .child(icon(&icons::CHECK, 12.0).color(k.c.success.icon))
                .child(status(t.up_to_date.into())),
        ),
        Update::Installing(version) => Some(
            row()
                .gap(6.0)
                .center_y()
                .child(Spinner::new().size(11.0).build(k))
                .child(status(fill(t.updating, version))),
        ),
    };
    let update = match &s.update {
        Update::Idle | Update::Failed(_) | Update::Latest => Some(
            Button::new(t.check_updates)
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut State| s.check_update())
                .build(k),
        ),
        Update::Available(version) => Some(buttons::button(
            k,
            Tone::Primary,
            &fill(t.update_to, version),
            Some(&icons::DOWNLOAD),
            |b| {
                b.help(t.update_help)
                    .on_press(|s: &mut State| s.install_update())
            },
        )),
        Update::Checking | Update::Installing(_) => None,
    };
    let line = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(
            col()
                .grow(1.0)
                .gap(2.0)
                .child(
                    text(format!("Fettle {}", crate::VERSION))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base),
                )
                .children(state),
        )
        .children(update)
        .child(
            Button::new(t.quit)
                .variant(ButtonVariant::Outline)
                .shortcut("⌘Q")
                .on_press_tx(|_: &mut State, tx| tx.quit())
                .build(k),
        );
    let mut block = col().w_full().padding(PAD).gap(8.0).child(line);
    if let Update::Failed(e) = &s.update {
        block = block.child(note(k, e, NoteKind::Failure));
    }
    block
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
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut State| s.open_apple_intelligence())
                .build(k),
        );
    }
    line
}

pub(super) fn chatgpt(s: &State, k: &Kit) -> El {
    let t = s.t();
    let line = row().w_full().center_y().gap(8.0);
    match (&s.settings.credentials.chatgpt, &s.form.login) {
        (_, Login::Waiting(_)) => line
            .child(
                text(t.finish_sign_in)
                    .font(k.ty.ui)
                    .color(k.c.text_muted)
                    .wrap()
                    .grow(1.0),
            )
            .child(
                Button::new(t.cancel)
                    .variant(ButtonVariant::Outline)
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
                    .variant(ButtonVariant::Outline)
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

/// The API's URL and key rows.
fn api(s: &State, k: &Kit) -> [El; 2] {
    let t = s.t();
    [
        wide(
            k,
            t.api_url,
            TextInput::new(&s.form.api_base)
                .label(t.api_url)
                .placeholder("https://api.openai.com/v1")
                .width(None)
                .on_edit(|s: &mut State| &mut s.form.api_base)
                .on_change(|s: &mut State, _| s.api_edited())
                .build(field_kit(k)),
        ),
        wide(
            k,
            t.api_key,
            TextInput::new(&s.form.api_key)
                .secure()
                .label(t.api_key)
                .placeholder(t.api_key_placeholder)
                .width(None)
                .on_edit(|s: &mut State| &mut s.form.api_key)
                .on_change(|s: &mut State, _| s.api_edited())
                .build(field_kit(k)),
        ),
    ]
}

/// The model dropdown, once the provider can list its models.
fn model(s: &State, k: &Kit, layer: &mut Vec<El>) -> Option<El> {
    let t = s.t();
    let current = s.settings.model()?;
    if Backend::of(&s.settings).is_err() {
        return None;
    }
    let loading = matches!(s.form.models, Models::Loading(_));
    let models: &[String] = match &s.form.models {
        Models::Ready(models) => models,
        _ => &[],
    };
    let current = Some(current).filter(|m| !m.is_empty());
    let select = Select::new("model", models)
        .label(t.model)
        .appearance(SelectAppearance::Field)
        .width(control(s))
        .current(current)
        .placeholder(if loading {
            t.loading_models
        } else {
            t.choose_model
        })
        .placement(Placement::BottomStart)
        .open(s.menu_open(Menu::Model) && !models.is_empty())
        .on_toggle(|s: &mut State| s.toggle_models())
        .on_close(|s: &mut State| s.close_menu())
        .on_choose(|s: &mut State, model, _| s.pick_model(model))
        .build(field_kit(k), layer);
    let Models::Failed(e) = &s.form.models else {
        return Some(wide(k, t.model, select));
    };
    Some(
        col().w_full().child(wide(k, t.model, select)).child(
            row()
                .w_full()
                .px(PAD)
                .pb(PAD)
                .center_y()
                .gap(8.0)
                .child(note(k, e, NoteKind::Failure).grow(1.0))
                .child(
                    Button::new(t.try_again)
                        .variant(ButtonVariant::Outline)
                        .on_press(|s: &mut State| s.load_models())
                        .build(k),
                ),
        ),
    )
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
        .child(
            Button::new(t.check_again)
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut State| s.recheck_accessibility())
                .build(k),
        )
        .child(buttons::button(k, Tone::Primary, t.allow, None, |b| {
            b.help(t.allow_help)
                .on_press(|s: &mut State| s.ask_accessibility())
        }))
}

/// A group's title, above it.
fn section(k: &Kit, title: &str) -> El {
    col().w_full().pt(8.0).pl(4.0).child(
        text(title.to_owned())
            .font(k.ty.text_small_medium)
            .color(k.c.text_muted),
    )
}

/// `rows` in a rounded box, with a line between each two.
fn group(k: &Kit, rows: Vec<El>) -> El {
    // In dark mode, a lift over the window: the dark base would hide the fields in it.
    let fill = if k.dark {
        k.light(0.05)
    } else {
        k.c.bg_base.alpha(0.85)
    };
    let mut card = col()
        .w_full()
        .radius(k.radius.lg)
        .fill(fill)
        .border(1.0, k.c.border_muted);
    for (i, line) in rows.into_iter().enumerate() {
        if i > 0 {
            card = card.child(divider(k));
        }
        card = card.child(line);
    }
    card
}

/// A row with `label` on the left and a wide `control` filling the column on its right.
fn wide(k: &Kit, label: &str, control: El) -> El {
    row()
        .w_full()
        .px(PAD)
        .py(8.0)
        .min_h(44.0)
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

/// A row with `label`, and `hint` under it, on the left, and a compact
/// `control` at the right edge.
fn compact(k: &Kit, label: &str, hint: Option<&str>, control: El) -> El {
    let hint = hint.map(|h| {
        text(h.to_owned())
            .font(k.ty.text_small)
            .color(k.c.text_muted)
            .wrap()
    });
    row()
        .w_full()
        .px(PAD)
        .py(8.0)
        .min_h(44.0)
        .center_y()
        .gap(12.0)
        .child(
            col()
                .grow(1.0)
                .gap(2.0)
                .child(
                    text(label.to_owned())
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .children(hint),
        )
        .child(control)
}
