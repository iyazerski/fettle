use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::mac;

const REPO: &str = "iyazerski/fettle";
const ASSET: &str = "Fettle.zip";
const BUNDLE_ID: &str = "dev.iyazerski.fettle";
/// Pin the release signing certificate (`MACOS_CERT_P12` in CI); rotate this hash with it.
const SIGNER: &str = r#"=identifier "dev.iyazerski.fettle" and certificate leaf = H"d92f0c1f8b4c244056244514e6e27e95cc431234""#;

pub async fn available(http: &reqwest::Client) -> Result<Option<String>> {
    let latest = latest(http).await?;
    Ok(newer(&latest, crate::VERSION)?.then_some(latest))
}

async fn latest(http: &reqwest::Client) -> Result<String> {
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

fn newer(candidate: &str, current: &str) -> Result<bool> {
    let parse =
        |v: &str| semver::Version::parse(v).with_context(|| format!("unexpected version {v}"));
    Ok(parse(candidate)? > parse(current)?)
}

pub async fn install(http: &reqwest::Client, version: &str) -> Result<PathBuf> {
    // A release would replace Fettle Dev.app with Fettle.app.
    ensure!(
        !crate::DEV,
        "Fettle Dev doesn't update itself: rebuild it with make dev"
    );
    let app = running_app()?;
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/{ASSET}");
    let get = async |url: &str| {
        http.get(url)
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("downloading {url}"))
    };
    let zip = get(&url).await?.bytes().await?;
    let sum = get(&format!("{url}.sha256")).await?.text().await?;
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
    tokio::task::spawn_blocking(move || swap(&app, &zip).map(|()| app)).await?
}

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

fn running_app() -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    mac::bundle_of(&exe)
        .map(Path::to_path_buf)
        .context("Fettle isn't running from Fettle.app, so it can't update itself")
}

fn swap(app: &Path, zip: &[u8]) -> Result<()> {
    let parent = app.parent().context("the app has no folder")?;
    // Beside the app, so the swap is a rename on one volume.
    let work = parent.join(format!(".fettle-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir(&work)
        .with_context(|| format!("Fettle can't write to {}", parent.display()))?;
    let result = unpack_and_swap(app, &work, zip);
    // After a swap, the old version is what's left in `work`.
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn unpack_and_swap(app: &Path, work: &Path, zip: &[u8]) -> Result<()> {
    let archive = work.join(ASSET);
    std::fs::write(&archive, zip)?;
    run(Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(&archive)
        .arg(work))?;
    let new = work.join("Fettle.app");
    ensure!(new.is_dir(), "the download has no Fettle.app");
    // The checksum comes from the same release, so it only catches damage. The
    // signature shows the app is Fettle's own.
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "-R", SIGNER])
        .arg(&new))
    .context("the download isn't signed by Fettle's release certificate")?;
    let adhoc = adhoc_signed(app);
    // One atomic exchange, so even a crash leaves one version in place.
    exchange(&new, app).context("putting the new version in place")?;
    if adhoc {
        // An ad-hoc signature changes with every build, so the old grant can't
        // match the new app: clear it, so macOS asks again.
        let _ = run(Command::new("/usr/bin/tccutil").args(["reset", "Accessibility", BUNDLE_ID]));
    }
    Ok(())
}

/// Swap the items at `a` and `b` in one step. Both must be on one volume.
fn exchange(a: &Path, b: &Path) -> std::io::Result<()> {
    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: both are NUL-terminated paths that outlive the call.
    if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

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
