use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::mac;
use crate::settings::home;

const LABEL: &str = crate::BUNDLE_ID;

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct LaunchAgent {
    label: &'static str,
    program_arguments: Vec<String>,
    run_at_load: bool,
    process_type: &'static str,
    standard_out_path: PathBuf,
    standard_error_path: PathBuf,
}

fn plist_path() -> PathBuf {
    home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

pub fn enabled() -> bool {
    plist_path().exists()
}

pub fn enable() -> Result<()> {
    let exe = std::env::current_exe().context("finding Fettle's executable")?;
    // From a bundle, start through LaunchServices, so opening Fettle.app later
    // reaches this copy instead of starting another.
    let args: Vec<String> = match mac::bundle_of(&exe) {
        Some(app) => vec![
            "/usr/bin/open".into(),
            "-g".into(),
            "-a".into(),
            app.to_string_lossy().into_owned(),
            "--args".into(),
            "--background".into(),
        ],
        None => vec![exe.to_string_lossy().into_owned(), "--background".into()],
    };
    let log = home().join(if crate::DEV {
        "Library/Logs/fettle-dev.log"
    } else {
        "Library/Logs/fettle.log"
    });
    let agent = LaunchAgent {
        label: LABEL,
        program_arguments: args,
        run_at_load: true,
        process_type: "Interactive",
        standard_out_path: log.clone(),
        standard_error_path: log,
    };
    let path = plist_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    plist::to_file_xml(&path, &agent).with_context(|| format!("writing {}", path.display()))
}

pub fn disable() -> Result<()> {
    match std::fs::remove_file(plist_path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).context("removing the login item")
        }
        _ => Ok(()),
    }
}
