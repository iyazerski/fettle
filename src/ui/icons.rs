//! The window's icons, as SVG in the subset Rune reads: 24-point outlines
//! with a 1.75 stroke.

use rune::Icon;

const fn icon(name: &'static str, body: &'static str) -> Icon {
    Icon {
        name,
        view: 24.0,
        body,
    }
}

pub static SPARKLE: Icon = icon(
    "sparkle",
    r#"<path d="M12 3.5l1.9 4.6a3 3 0 0 0 1.6 1.6l4.6 1.9-4.6 1.9a3 3 0 0 0-1.6 1.6L12 19.7l-1.9-4.6a3 3 0 0 0-1.6-1.6l-4.6-1.9 4.6-1.9a3 3 0 0 0 1.6-1.6z" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/>"#,
);
pub static GLOBE: Icon = icon(
    "globe",
    r#"<circle cx="12" cy="12" r="8.75" fill="none" stroke="currentColor" stroke-width="1.75"/><path d="M3.5 12h17M12 3.25c2.3 2.4 3.5 5.3 3.5 8.75s-1.2 6.35-3.5 8.75c-2.3-2.4-3.5-5.3-3.5-8.75s1.2-6.35 3.5-8.75z" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/>"#,
);
pub static GEAR: Icon = icon(
    "gear",
    r#"<circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="1.75"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/>"#,
);
pub static CLOSE: Icon = icon(
    "close",
    r#"<path d="M6.5 6.5l11 11M17.5 6.5l-11 11" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/>"#,
);
pub static BACK: Icon = icon(
    "back",
    r#"<path d="M14.75 17.5 9.25 12 14.75 6.5" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static CHEV_DOWN: Icon = icon(
    "chev-down",
    r#"<path d="M17.5 9.25 12 14.75 6.5 9.25" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static COPY: Icon = icon(
    "copy",
    r#"<rect x="8.5" y="8.5" width="11.5" height="11.5" rx="2.5" fill="none" stroke="currentColor" stroke-width="1.75"/><path d="M15.5 8.5V6.5a2.5 2.5 0 0 0-2.5-2.5H6.5A2.5 2.5 0 0 0 4 6.5V13a2.5 2.5 0 0 0 2.5 2.5h2" fill="none" stroke="currentColor" stroke-width="1.75"/>"#,
);
pub static CHECK: Icon = icon(
    "check",
    r#"<path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static REPLACE: Icon = icon(
    "replace",
    r#"<path d="M19 5v6.5a3 3 0 0 1-3 3H5.5M9.5 10.5l-4 4 4 4" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static ALERT: Icon = icon(
    "alert",
    r#"<circle cx="12" cy="12" r="8.75" fill="none" stroke="currentColor" stroke-width="1.75"/><path d="M12 7.75v5" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"/><circle cx="12" cy="16.25" r="1.1" fill="currentColor"/>"#,
);
pub static PEN: Icon = icon(
    "pen",
    r#"<path d="M4.75 19.25h4l10-10a2.12 2.12 0 0 0-3-3l-10 10zM13.75 8.25l2 2" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static PIN: Icon = icon(
    "pin",
    r#"<path d="M8.5 3.75h7M9.75 3.75v5.5L6.5 13.5h11l-3.25-4.25v-5.5M12 13.5v6.75" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static STOP: Icon = icon(
    "stop",
    r#"<rect x="7" y="7" width="10" height="10" rx="2.5" fill="currentColor"/>"#,
);
pub static STAR: Icon = icon(
    "star",
    r#"<path d="M12 3.6l2.6 5.3 5.8.85-4.2 4.1 1 5.8L12 16.9l-5.2 2.75 1-5.8-4.2-4.1 5.8-.85z" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/>"#,
);
pub static STAR_FILLED: Icon = icon(
    "star-filled",
    r#"<path d="M12 3.6l2.6 5.3 5.8.85-4.2 4.1 1 5.8L12 16.9l-5.2 2.75 1-5.8-4.2-4.1 5.8-.85z" fill="currentColor" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/>"#,
);
pub static DOWNLOAD: Icon = icon(
    "download",
    r#"<path d="M12 4v11m-4.5-4.5L12 15l4.5-4.5M5 19.5h14" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
