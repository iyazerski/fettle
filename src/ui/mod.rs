//! A nonactivating panel keeps the source app active so it can take the pasted result.

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

/// Keep the window on the main thread, as AppKit requires.
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
    settings.credentials = keychain::load();

    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        on_key: Some(on_key),
        ..AppSpec::new(
            crate::NAME,
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
        .init(move || State::new(me, handle, http, settings, settings_path))
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
    Ok(())
}

/// Hold until exit so login-item and manual launches can't compete for the shortcut.
fn single_instance() -> Result<std::fs::File> {
    let path = Settings::path().with_file_name("fettle.lock");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file =
        std::fs::File::create(&path).with_context(|| format!("opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => {
            anyhow::bail!("{} is already running", crate::NAME)
        }
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("locking {}", path.display()))
        }
    }
}
