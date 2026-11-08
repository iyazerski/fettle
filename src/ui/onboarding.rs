//! The welcome steps on the first run: what Fettle does, where the model runs,
//! the Accessibility permission, and a first try on a sample.

use rune::{Cursor, El, Shortcut, TextStyle, col, icon, row, spacer, text};
use rune_kit::{Button, ButtonVariant, Kit};

use super::buttons::{self, Tone};
use super::icons;
use super::settings_view::{accessibility, chatgpt, on_device};
use super::state::{Page, State};
use super::view::{Lead, header};
use crate::i18n::fill;
use crate::settings::Provider;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Model,
    Access,
    Done,
}

impl Step {
    const ALL: [Step; 4] = [Step::Welcome, Step::Model, Step::Access, Step::Done];

    fn index(self) -> usize {
        self as usize
    }
}

impl State {
    /// Show the welcome steps, where they were left, or from the start.
    pub fn open_onboarding(&mut self) {
        self.recheck_accessibility();
        self.recheck_on_device();
        self.menu = None;
        if !matches!(self.page, Page::Onboarding(_)) {
            self.page = Page::Onboarding(Step::Welcome);
        }
        self.show();
    }

    fn go_to(&mut self, step: Step) {
        self.page = Page::Onboarding(step);
    }

    /// Go one step forward, or back if `forward` is false.
    fn step(&mut self, forward: bool) {
        let Page::Onboarding(step) = self.page else {
            return;
        };
        let at = step.index();
        let next = if forward {
            at + 1
        } else {
            at.saturating_sub(1)
        };
        if let Some(step) = Step::ALL.get(next) {
            self.go_to(*step);
        }
    }

    /// Finish the welcome steps: open the main page on the sample if `try_it`,
    /// else hide until the shortcut.
    fn finish_onboarding(&mut self, try_it: bool) {
        self.settings.onboarded = true;
        self.save();
        self.page = Page::Main;
        if try_it {
            self.answer = None;
            self.notice = None;
            self.source.replace_text(self.t().sample.to_owned());
        } else {
            self.hide();
        }
    }
}

pub fn view(s: &State, k: &Kit, step: Step) -> El {
    let body = match step {
        Step::Welcome => welcome(s, k),
        Step::Model => model(s, k),
        Step::Access => access(s, k),
        Step::Done => done(s, k),
    };
    // Back from every step after the first.
    let lead = if step == Step::Welcome {
        Lead::Mark
    } else {
        Lead::Back(|s| s.step(false))
    };
    let dots = row().gap(6.0).center_y().children(Step::ALL.map(|at| {
        let color = if at.index() <= step.index() {
            k.c.bg_accent
        } else {
            k.c.text_faint.alpha(0.5)
        };
        col().w(6.0).h(6.0).radius(3.0).fill(color)
    }));
    col()
        .w_full()
        .gap(16.0)
        .child(header(s, k, lead, "Fettle", vec![dots]))
        .child(body)
}

/// A step's title, text and content, with its buttons at the bottom right.
fn page(k: &Kit, title: &str, body: &str, content: Option<El>, buttons: Vec<El>) -> El {
    let heading = TextStyle {
        size: 19.0,
        line_height: 26.0,
        ..k.ty.ui_medium
    };
    col()
        .w_full()
        .gap(10.0)
        .child(text(title).font(heading).color(k.c.text_base).wrap())
        .children((!body.is_empty()).then(|| text(body).font(k.ty.ui).color(k.c.text_muted).wrap()))
        .children(content.map(|c| col().w_full().pt(6.0).child(c)))
        .child(
            row()
                .w_full()
                .pt(10.0)
                .gap(8.0)
                .center_y()
                .child(spacer())
                .children(buttons),
        )
}

fn next(k: &Kit, label: &str) -> El {
    buttons::button(k, Tone::Primary, label, None, |b| {
        b.on_press(|s: &mut State| s.step(true))
    })
}

/// The shortcut as keycaps, like ⌃⌥E.
fn shortcut(s: &State) -> String {
    s.settings
        .shortcut
        .parse::<Shortcut>()
        .map_or_else(|_| s.settings.shortcut.clone(), |sc| sc.keycaps().concat())
}

fn welcome(s: &State, k: &Kit) -> El {
    let t = s.t();
    page(
        k,
        t.welcome_title,
        &fill(t.welcome_body, shortcut(s)),
        None,
        vec![next(k, t.get_started)],
    )
}

fn model(s: &State, k: &Kit) -> El {
    let t = s.t();
    let choices = col()
        .w_full()
        .gap(8.0)
        .child(choice(
            s,
            k,
            Provider::OnDevice,
            t.on_device,
            t.on_device_body,
        ))
        .child(choice(
            s,
            k,
            Provider::ChatGpt,
            t.chatgpt_plan,
            t.chatgpt_body,
        ))
        .child(choice(s, k, Provider::Api, t.compatible_api, t.api_body));
    page(k, t.model_title, "", Some(choices), vec![next(k, t.next)])
}

/// A card for `provider` that picks it when pressed; the picked one shows
/// whether it's ready.
fn choice(s: &State, k: &Kit, provider: Provider, title: &str, body: &str) -> El {
    let picked = s.settings.provider == provider;
    let (edge, fill) = if picked {
        (k.c.bg_accent, k.c.bg_accent.alpha(0.08))
    } else {
        (k.c.border_muted, k.c.bg_base.alpha(0.6))
    };
    let detail = match provider {
        _ if !picked => None,
        Provider::OnDevice => Some(on_device(s, k)),
        Provider::ChatGpt => Some(chatgpt(s, k)),
        Provider::Api => None,
    };
    col()
        .key(format!("choice-{provider:?}"))
        .role("radio")
        .label(title)
        .w_full()
        .padding(12.0)
        .gap(4.0)
        .radius(k.radius.lg)
        .border(1.0, edge)
        .fill(fill)
        .cursor(Cursor::Pointer)
        .on_press(move |s: &mut State| s.set_provider(provider))
        .child(
            row()
                .w_full()
                .center_y()
                .gap(6.0)
                .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
                .child(spacer())
                .children(picked.then(|| icon(&icons::CHECK, 14.0).color(k.c.icon_accent))),
        )
        .child(
            text(body)
                .font(k.ty.text_small)
                .color(k.c.text_muted)
                .wrap(),
        )
        .children(detail.map(|d| col().w_full().pt(6.0).child(d)))
}

fn access(s: &State, k: &Kit) -> El {
    let t = s.t();
    page(
        k,
        t.access_title,
        t.access_body,
        Some(accessibility(s, k)),
        vec![next(k, t.next)],
    )
}

fn done(s: &State, k: &Kit) -> El {
    let t = s.t();
    page(
        k,
        t.done_title,
        &fill(t.done_body, shortcut(s)),
        None,
        vec![
            Button::new(t.done)
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut State| s.finish_onboarding(false))
                .build(k),
            buttons::button(k, Tone::Primary, t.try_it, Some(&icons::SPARKLE), |b| {
                b.on_press(|s: &mut State| s.finish_onboarding(true))
            }),
        ],
    )
}
