//! Capsule buttons: a solid accent one for the main action and a quiet one
//! for the rest.

use rune::{Color, El, Icon, Shadow, icon, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit};

/// The capsule's height, as a large push button's, and so its corner radius.
const HEIGHT: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The accent fill, as a switch that is on: the main action.
    Primary,
    /// A quiet fill with a hairline edge: the other actions.
    Secondary,
}

/// A capsule button labelled `label`, with `glyph` before it. `configure` adds
/// the press, shortcut and help.
pub fn button(
    k: &Kit,
    tone: Tone,
    label: &str,
    glyph: Option<&'static Icon>,
    configure: impl FnOnce(Button) -> Button,
) -> El {
    let ink = match tone {
        Tone::Primary => Color::WHITE,
        Tone::Secondary => k.c.text_base,
    };
    let caption = label.to_owned();
    let button = Button::new(label)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label_with(move |font, _| {
            row()
                .gap(6.0)
                .center_y()
                .children(glyph.map(|g| icon(g, 15.0).color(ink)))
                .child(text(caption).font(font).color(ink))
        });
    surface(k, tone, configure(button).build(k).px(14.0)).press_scale(0.98)
}

/// Paint `e` as a capsule in `tone`; it highlights as one while hovered.
pub fn surface(k: &Kit, tone: Tone, e: El) -> El {
    let c = &k.c;
    let shadow = |alpha: f32| Shadow {
        y: 1.0,
        blur: 2.0,
        color: Color::BLACK.alpha(alpha),
        ..Shadow::default()
    };
    let e = e.radius(HEIGHT / 2.0);
    match tone {
        Tone::Primary => e
            .fill(c.bg_accent)
            .box_shadow(shadow(0.18))
            .hover(k.on(c.contrast_hover, c.bg_accent))
            .press(k.on(c.contrast_pressed, c.bg_accent)),
        Tone::Secondary if k.dark => e
            .fill(Color::WHITE.alpha(0.07))
            .border(0.5, Color::WHITE.alpha(0.1))
            .box_shadow(shadow(0.25))
            .hover(Color::WHITE.alpha(0.06))
            .press(Color::WHITE.alpha(0.1)),
        Tone::Secondary => e
            .fill(Color::WHITE.alpha(0.8))
            .border(0.5, Color::BLACK.alpha(0.1))
            .box_shadow(shadow(0.06))
            .hover(Color::BLACK.alpha(0.04))
            .press(Color::BLACK.alpha(0.08)),
    }
}
