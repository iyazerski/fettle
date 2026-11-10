//! The floating window: a nonactivating panel, so the app the text came from
//! stays active and can take the result back. A global shortcut copies the
//! selection and opens it; model calls run on a small tokio runtime beside it.

mod buttons;
mod checker;
mod client;
mod dictating;
mod draft;
mod icons;
mod menu_bar;
mod onboarding;
mod pending;
mod saver;
mod settings_state;
mod settings_view;
mod state;
mod view;

use anyhow::{Context, Result};
use objc2_app_kit::{NSFloatingWindowLevel, NSWindowCollectionBehavior};
use rune::{App, AppSpec, MacWindowOptions, RunOptions, WindowOptions, WindowSpec};

use crate::keychain;
use crate::settings::Settings;

pub use self::state::Start;
use self::state::State;
use self::view::{CORNER, WIDTH, on_key, view};

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
    let mut settings = Settings::load(&settings_path);
    let credentials = keychain::load();
    settings.credentials = credentials.clone();

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
    let frames = me.clone();
    let handle = rt.handle().clone();
    me.update(move |s, _| s.started(start));
    let app = app
        .init(move || State::new(me, handle, http, settings, settings_path, credentials))
        .on_focus(|s, focused, _| {
            if focused {
                // Back from System Settings, where Accessibility or Apple
                // Intelligence may have just been turned on.
                s.recheck_accessibility();
                s.recheck_on_device();
            } else {
                s.blurred();
            }
        })
        // Also on logout and shutdown, which skip Fettle's own Quit.
        .on_quit(|s, _| s.quitting())
        .on_reopen(|s, _, _| {
            s.open_settings();
            true
        })
        .on_frame_drawn({
            let mut size = (0.0, 0.0);
            move |rt| {
                if rt.viewport != size {
                    size = rt.viewport;
                    frames.update_if(move |s, tx| {
                        s.laid_out(size, tx);
                        false
                    });
                }
            }
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
                // Above normal windows, under menus.
                .level(NSFloatingWindowLevel)
                // On every Space, and over full-screen apps.
                .collection_behavior(
                    (NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::FullScreenAuxiliary)
                        .0,
                )
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
    let path = Settings::path().with_file_name("fettle.lock");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file =
        std::fs::File::create(&path).with_context(|| format!("opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!("Fettle is already running"),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("locking {}", path.display()))
        }
    }
}
