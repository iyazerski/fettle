//! The floating window: a nonactivating panel, so the app the text came from
//! stays active and can take the result back. A global shortcut copies the
//! selection and opens it; model calls run on a small tokio runtime beside it.

mod buttons;
mod icons;
mod settings_state;
mod settings_view;
mod state;
mod view;

use anyhow::{Context, Result};
use rune::{App, AppSpec, MacWindowOptions, RunOptions, WindowOptions, WindowSpec};

use crate::settings::Settings;

pub use self::state::Start;
use self::state::State;
use self::view::{CORNER, WIDTH, on_key, view};

/// NSWindow collection behavior: on every Space, and over full-screen apps.
const ALL_SPACES: usize = 1 << 0;
const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
/// NSFloatingWindowLevel: above normal windows, under menus.
const FLOATING: isize = 3;

/// Run the app until it quits. The window owns the main thread, as macOS needs.
pub fn run(start: Start) -> Result<()> {
    let _only = single_instance()?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("starting the runtime")?;
    let http = crate::llm::http()?;
    let settings_path = Settings::path();
    let settings = Settings::load(&settings_path);

    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        on_key: Some(on_key),
        ..AppSpec::new(
            "Fettle",
            (WIDTH, 240.0),
            || unreachable!("init() below builds the state"),
            view,
        )
    };
    let app = App::new(spec)
        .windowless()
        .keep_running()
        .fit_content_height();
    let me = app.handle();
    let handle = rt.handle().clone();
    me.update(move |s, _| s.started(start));
    let app = app
        .init(move || State::new(me, handle, http, settings, settings_path))
        .on_focus(|s, focused, _| {
            if !focused {
                s.blurred();
            }
        })
        .on_reopen(|s, _, _| {
            s.open_settings();
            true
        });
    rune::run_windows_with_args(RunOptions { accessory: true }, [], move |_| {
        let options = WindowOptions {
            transparent: true,
            borderless: true,
            resizable: false,
            visible: false,
            min_size: Some((0.0, 0.0)),
            ..Default::default()
        }
        .macos(
            MacWindowOptions::default()
                .nonactivating_panel(true)
                .level(FLOATING)
                .collection_behavior(ALL_SPACES | FULL_SCREEN_AUXILIARY)
                .corner_radius(CORNER),
        );
        vec![WindowSpec::from_app("fettle", app).configured_options(options)]
    });
    drop(rt);
    Ok(())
}

/// Hold a lock for as long as Fettle runs, so a second copy (the login item and
/// a manual start) quits instead of fighting over the shortcut.
fn single_instance() -> Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    let path = Settings::path().with_file_name("fettle.lock");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file =
        std::fs::File::create(&path).with_context(|| format!("opening {}", path.display()))?;
    // SAFETY: flock on a descriptor this function owns.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        anyhow::bail!("Fettle is already running");
    }
    Ok(file)
}
