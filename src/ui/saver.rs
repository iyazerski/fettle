//! Saving the settings off the UI thread: the file and the keychain can be slow,
//! and a save that's still waiting gives way to a newer one.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use rune::AppHandle;
use tokio::runtime::Handle;
use tokio::sync::watch;

use super::state::State;
use crate::keychain;
use crate::settings::{Credentials, Settings};

/// Sends each change of the settings to a task that writes them in order.
pub struct Saver {
    /// The newest settings to save, numbered by revision.
    latest: watch::Sender<(u64, Settings)>,
    store: Arc<Store>,
}

impl Saver {
    /// Start saving `settings`' changes to `path` on `rt`, where the keychain
    /// holds `saved`, and report failures to `app`.
    pub fn new(
        rt: &Handle,
        app: AppHandle<State>,
        settings: &Settings,
        path: PathBuf,
        saved: Credentials,
    ) -> Self {
        let store = Arc::new(Store {
            path,
            written: Mutex::new((0, saved)),
        });
        let (latest, mut changes) = watch::channel((0, settings.clone()));
        let writer = Arc::clone(&store);
        rt.spawn(async move {
            while changes.changed().await.is_ok() {
                let (revision, settings) = changes.borrow_and_update().clone();
                let store = Arc::clone(&writer);
                let written =
                    tokio::task::spawn_blocking(move || store.write(revision, &settings)).await;
                if let Err(e) = written.map_err(anyhow::Error::from).and_then(|r| r) {
                    app.update(move |s, _| s.save_failed(&e));
                }
            }
        });
        Self { latest, store }
    }

    /// Save `settings` soon.
    pub fn save(&self, settings: &Settings) {
        self.latest.send_modify(|(revision, latest)| {
            *revision += 1;
            latest.clone_from(settings);
        });
    }

    /// Save the newest settings now, unless they're saved already: Fettle is
    /// about to quit, and the task would not get to them.
    pub fn flush(&self) -> Result<()> {
        let latest = self.latest.borrow();
        self.store.write(latest.0, &latest.1)
    }
}

/// Where the settings go, and the revision and credentials last written there.
struct Store {
    path: PathBuf,
    written: Mutex<(u64, Credentials)>,
}

impl Store {
    /// Write revision `revision` of the settings, unless a later one is written already.
    fn write(&self, revision: u64, settings: &Settings) -> Result<()> {
        let mut written = self.written.lock().expect("settings store lock poisoned");
        if revision <= written.0 {
            return Ok(());
        }
        settings.save(&self.path)?;
        keychain::save(&settings.credentials, &written.1)?;
        *written = (revision, settings.credentials.clone());
        Ok(())
    }
}
