//! Self-update: find the latest GitHub release, and swap the running
//! Quill.app for it.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

const REPO: &str = "iyazerski/quill";
const ASSET: &str = "Quill.zip";
const BUNDLE_ID: &str = "dev.iyazerski.quill";
/// The code requirement every release meets: Quill's bundle ID, signed with the
/// release certificate (`MACOS_CERT_P12` in CI). A new certificate needs a new hash here.
const SIGNER: &str = r#"=identifier "dev.iyazerski.quill" and certificate leaf = H"777f4422da3703fbf64916ee0909d0b3eaf0b37c""#;

/// The version of the latest release, like `0.1.7`.
pub async fn latest(http: &reqwest::Client) -> Result<String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    let release: Release = http
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("reading the latest release")?;
    Ok(release.tag_name.trim_start_matches('v').to_owned())
}

/// Whether version `candidate` comes after `current`, comparing dotted numbers.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parts(candidate) > parts(current)
}

/// Download release `version` and put it in place of the running Quill.app.
/// Returns the app's path, to start it again.
pub async fn install(http: &reqwest::Client, version: &str) -> Result<PathBuf> {
    let app = running_app()?;
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/{ASSET}");
    let get = |url: String| async move {
        http.get(&url)
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("downloading {url}"))
    };
    let zip = get(url.clone()).await?.bytes().await?;
    let sum = get(format!("{url}.sha256")).await?.text().await?;
    let expected = sum.split_whitespace().next().unwrap_or_default();
    let actual: String = ring::digest::digest(&ring::digest::SHA256, &zip)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    ensure!(
        actual == expected,
        "the download is damaged: its checksum doesn't match"
    );
    let target = app.clone();
    tokio::task::spawn_blocking(move || swap(&target, &zip)).await??;
    Ok(app)
}

/// Start `app` once this process has quit.
pub fn relaunch(app: &Path) -> Result<()> {
    Command::new("/bin/sh")
        .args([
            "-c",
            r#"while kill -0 "$1" 2>/dev/null; do sleep 0.1; done; exec /usr/bin/open "$0""#,
        ])
        .arg(app)
        .arg(std::process::id().to_string())
        .spawn()
        .context("starting the new version")?;
    Ok(())
}

/// The Quill.app this binary runs from.
fn running_app() -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    // Quill.app/Contents/MacOS/quill
    exe.ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
        .context("Quill isn't running from Quill.app, so it can't update itself")
}

/// Unpack `zip` next to `app` and swap the two, keeping the old app if the
/// swap fails.
fn swap(app: &Path, zip: &[u8]) -> Result<()> {
    let parent = app.parent().context("the app has no folder")?;
    // Beside the app, so the swap is a rename on one volume.
    let work = parent.join(format!(".quill-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir(&work)
        .with_context(|| format!("Quill can't write to {}", parent.display()))?;
    let result = (|| {
        let archive = work.join(ASSET);
        std::fs::write(&archive, zip)?;
        run(Command::new("/usr/bin/ditto")
            .arg("-x")
            .arg("-k")
            .arg(&archive)
            .arg(&work))?;
        let new = work.join("Quill.app");
        ensure!(new.is_dir(), "the download has no Quill.app");
        // The checksum comes from the same release, so it only catches damage. The
        // signature shows the app is Quill's own.
        run(Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", "-R", SIGNER])
            .arg(&new))
        .context("the download isn't signed by Quill's release certificate")?;
        let adhoc = adhoc_signed(app);
        let old = work.join("old.app");
        std::fs::rename(app, &old).context("moving the old version away")?;
        if let Err(e) = std::fs::rename(&new, app) {
            std::fs::rename(&old, app)?;
            return Err(e).context("putting the new version in place");
        }
        if adhoc {
            // An ad-hoc signature changes with every build, so the old grant can't
            // match the new app: clear it, so macOS asks again.
            let _ =
                run(Command::new("/usr/bin/tccutil").args(["reset", "Accessibility", BUNDLE_ID]));
        }
        Ok(())
    })();
    // Without an app in place, a failed rollback left the old one in `work`.
    if !app.exists() {
        return result
            .with_context(|| format!("the old version is in {}", work.join("old.app").display()));
    }
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Whether `app` has an ad-hoc signature, which macOS's privacy grants can't follow
/// from one build to the next.
fn adhoc_signed(app: &Path) -> bool {
    Command::new("/usr/bin/codesign")
        .arg("-dv")
        .arg(app)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stderr).contains("Signature=adhoc"))
}

fn run(command: &mut Command) -> Result<()> {
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "{:?} failed: {}",
            command.get_program(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn compares_versions_by_number() {
        for (candidate, current, expected) in [
            ("0.1.10", "0.1.9", true),
            ("0.1.9", "0.1.10", false),
            ("0.1.5", "0.1.5", false),
            ("0.2.0", "0.1.99", true),
            ("1.0", "0.9.9", true),
        ] {
            assert_eq!(
                newer(candidate, current),
                expected,
                "{candidate} vs {current}"
            );
        }
    }
}
