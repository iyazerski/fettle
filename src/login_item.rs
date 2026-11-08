//! Start at login: a per-user LaunchAgent that runs the app in the background.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::mac;
use crate::settings::home;

const LABEL: &str = "dev.iyazerski.fettle";

fn plist_path() -> PathBuf {
    home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

pub fn enabled() -> bool {
    plist_path().exists()
}

/// Start this executable, hidden, at every login. It takes effect from the
/// next login: the running copy is already the one in the background.
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
    let args: String = args
        .iter()
        .map(|a| format!("\n    <string>{}</string>", xml_escape(a)))
        .collect();
    let log = home().join("Library/Logs/fettle.log");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>{args}
  </array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        log = xml_escape(&log.to_string_lossy()),
    );
    let path = plist_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, plist).with_context(|| format!("writing {}", path.display()))
}

/// Stop starting at login. The running copy keeps running.
pub fn disable() -> Result<()> {
    match std::fs::remove_file(plist_path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).context("removing the login item")
        }
        _ => Ok(()),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
