//! Fettle: select text in any Mac app, press a shortcut, and fix or
//! translate it in a small floating window.
//!
//! The window ([`ui`]) grabs the selection through the pasteboard ([`mac`]),
//! asks the configured model ([`llm`]) with the [`prompts`], and shows the
//! answer, with a word diff ([`diff`]) for fixes.

mod ax;
mod dictation;
mod diff;
mod document;
mod hyphens;
mod i18n;
mod json_file;
mod keychain;
mod llm;
mod login_item;
mod mac;
mod prompts;
mod rich;
mod settings;
mod ui;
mod underlines;
mod update;

use anyhow::Result;
use clap::Parser;

/// The release (CI sets it), else the crate version.
pub const VERSION: &str = match option_env!("FETTLE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// Debug builds are Fettle Dev, with their own name, bundle ID, settings,
/// keychain items, login item and shortcut, so they run beside the installed app.
/// `scripts/bundle.sh` gives Fettle Dev.app the same name and ID.
pub const DEV: bool = cfg!(debug_assertions);
pub const NAME: &str = if DEV { "Fettle Dev" } else { "Fettle" };
pub const BUNDLE_ID: &str = if DEV {
    "dev.iyazerski.fettle.dev"
} else {
    "dev.iyazerski.fettle"
};

#[derive(Parser)]
#[command(version = VERSION, about, long_about = None)]
struct Cli {
    /// Start hidden and wait for the shortcut (how it runs at login)
    #[arg(long)]
    background: bool,
}

fn main() -> Result<()> {
    rune::prewarm_gpu();
    let cli = Cli::parse();
    ui::run(if cli.background {
        ui::Start::Hidden
    } else {
        ui::Start::Shown
    })
}
