//! Quill: select text in any Mac app, press a shortcut, and fix or
//! translate it in a small floating window.
//!
//! The window ([`ui`]) grabs the selection through the pasteboard ([`mac`]),
//! asks the configured model ([`llm`]) with the [`prompts`], and shows the
//! answer, with a word diff ([`diff`]) for fixes.

mod diff;
mod json_file;
mod llm;
mod login_item;
mod mac;
mod prompts;
mod settings;
mod ui;
mod update;

use anyhow::Result;
use clap::Parser;

/// The release (CI sets it), else the crate version.
pub const VERSION: &str = match option_env!("QUILL_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
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
    ui::run(ui::Opts {
        background: cli.background,
    })
}
