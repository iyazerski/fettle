//! Save off the UI thread; queued changes give way to the newest settings.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use rune::AppHandle;
use tokio::runtime::Handle;
use tokio::sync::watch;

use super::state::State;
use crate::keychain;
use crate::settings::{Credentials, Settings};

pub struct Saver {
    latest: watch::Sender<(u64, Settings)>,
    store: Arc<Store>,
}

impl Saver {
    pub fn new(rt: &Handle, app: AppHandle<State>, settings: &Settings, path: PathBuf) -> Self {
        let store = Arc::new(Store {
            path,
            written: Mutex::new((0, settings.credentials.clone())),
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

    pub fn save(&self, settings: &Settings) {
        self.latest.send_modify(|(revision, latest)| {
            *revision += 1;
            latest.clone_from(settings);
        });
    }

    /// Flush synchronously on quit, before the background task loses its runtime.
    pub fn flush(&self) -> Result<()> {
        let latest = self.latest.borrow();
        self.store.write(latest.0, &latest.1)
    }
}

struct Store {
    path: PathBuf,
    written: Mutex<(u64, Credentials)>,
}

impl Store {
    /// Ignore revisions already written by a newer background save or quit-time flush.
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
