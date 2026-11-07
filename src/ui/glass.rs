//! Buttons in a glass look: a translucent capsule with a sheen down its face,
//! a lit top rim and a soft shadow. Drawn by Rune rather than with native
//! Liquid Glass, which a window only gets a few of.

use rune::{Color, El, Icon, Shadow, icon, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit};

/// The capsule's height, as a large push button's, and so its corner radius.
const HEIGHT: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Tinted with the accent: the main action.
    Tinted,
    /// Clear: the other actions.
    Clear,
}

/// A glass button labelled `label`, with `glyph` before it. `configure` adds
/// the press, shortcut and help.
pub fn button(
    k: &Kit,
    tone: Tone,
    label: &str,
    glyph: Option<&'static Icon>,
    configure: impl FnOnce(Button) -> Button,
) -> El {
    let ink = match tone {
        Tone::Tinted => Color::WHITE,
        Tone::Clear => k.c.text_base,
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
    surface(k, tone, configure(button).build(k).px(14.0)).press_scale(0.97)
}

/// Paint `e` as a glass capsule.
pub fn surface(k: &Kit, tone: Tone, e: El) -> El {
    let white = Color::WHITE;
    let black = Color::BLACK;
    let inset = |y: f32, blur: f32, color: Color| Shadow {
        y,
        blur,
        color,
        inset: true,
        ..Shadow::default()
    };
    let drop = |y: f32, blur: f32, color: Color| Shadow {
        y,
        blur,
        color,
        ..Shadow::default()
    };
    let e = e.radius(HEIGHT / 2.0);
    match tone {
        Tone::Tinted => {
            // The light theme's accent in both: the dark theme's is too pale under white text.
            let accent = rune_theme_opencode::kit(false).c.text_accent;
            e.fill(accent)
                .gradient(accent.mix(white, 0.28), accent.mix(black, 0.06))
                .border(0.5, white.alpha(0.3))
                .box_shadow(inset(1.0, 1.0, white.alpha(0.45)))
                .box_shadow(inset(-1.0, 2.0, black.alpha(0.12)))
                .box_shadow(drop(2.0, 8.0, accent.alpha(0.35)))
                .box_shadow(drop(0.5, 1.0, black.alpha(0.12)))
                .hover(white.alpha(0.12))
                .press(black.alpha(0.1))
        }
        Tone::Clear if k.dark => e
            .fill(white.alpha(0.08))
            .gradient(white.alpha(0.14), white.alpha(0.04))
            .border(0.5, white.alpha(0.12))
            .box_shadow(inset(1.0, 1.0, white.alpha(0.16)))
            .box_shadow(drop(1.0, 4.0, black.alpha(0.3)))
            .hover(white.alpha(0.06))
            .press(white.alpha(0.1)),
        Tone::Clear => e
            .fill(white.alpha(0.55))
            .gradient(white.alpha(0.8), white.alpha(0.4))
            .border(0.5, black.alpha(0.08))
            .box_shadow(inset(1.0, 1.0, white.alpha(0.9)))
            .box_shadow(drop(1.0, 4.0, black.alpha(0.08)))
            .hover(black.alpha(0.04))
            .press(black.alpha(0.08)),
    }
}
