//! The credentials Fettle keeps in the login keychain rather than the settings
//! file: the ChatGPT sign-in and the API key.

use anyhow::{Context, Result};
use security_framework::base::Error;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

use crate::settings::{ChatGptAuth, Settings};

/// Debug builds keep their own items, so a test copy can't change the
/// installed app's sign-in.
const SERVICE: &str = if cfg!(debug_assertions) {
    "dev.iyazerski.fettle.debug"
} else {
    "dev.iyazerski.fettle"
};
const CHATGPT: &str = "chatgpt";
const API_KEY: &str = "api-key";
/// `errSecItemNotFound`.
const NOT_FOUND: i32 = -25300;

/// The credentials in the settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Credentials {
    pub chatgpt: Option<ChatGptAuth>,
    pub api_key: String,
}

impl Credentials {
    /// Return the credentials `settings` hold.
    pub fn of(settings: &Settings) -> Self {
        Self {
            chatgpt: settings.chatgpt.clone(),
            api_key: settings.api_key.clone(),
        }
    }

    /// Put these credentials into `settings`.
    pub fn apply(&self, settings: &mut Settings) {
        settings.chatgpt = self.chatgpt.clone();
        settings.api_key = self.api_key.clone();
    }
}

/// Read the credentials; one that can't be read is reported and left out.
pub fn load() -> Credentials {
    let chatgpt = read(CHATGPT)
        .and_then(|bytes| {
            bytes
                .map(|b| serde_json::from_slice(&b).context("reading the ChatGPT sign-in"))
                .transpose()
        })
        .unwrap_or_else(|e| {
            eprintln!("{e:#}");
            None
        });
    let api_key = read(API_KEY)
        .map(|bytes| String::from_utf8_lossy(&bytes.unwrap_or_default()).into_owned())
        .unwrap_or_else(|e| {
            eprintln!("{e:#}");
            String::new()
        });
    Credentials { chatgpt, api_key }
}

/// Write the credentials in `new` that differ from `old`, what the keychain holds.
pub fn save(new: &Credentials, old: &Credentials) -> Result<()> {
    if new.chatgpt != old.chatgpt {
        let json = new.chatgpt.as_ref().map(serde_json::to_vec).transpose()?;
        write(CHATGPT, json.as_deref())?;
    }
    if new.api_key != old.api_key {
        write(
            API_KEY,
            Some(new.api_key.as_bytes()).filter(|k| !k.is_empty()),
        )?;
    }
    Ok(())
}

/// Return the item for `account`, if there is one.
fn read(account: &str) -> Result<Option<Vec<u8>>> {
    match get_generic_password(SERVICE, account) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.code() == NOT_FOUND => Ok(None),
        Err(e) => Err(failed(e, "reading", account)),
    }
}

/// Store `secret` for `account`, or delete the item when it's `None`.
fn write(account: &str, secret: Option<&[u8]>) -> Result<()> {
    let result = match secret {
        Some(secret) => set_generic_password(SERVICE, account, secret),
        None => match delete_generic_password(SERVICE, account) {
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            other => other,
        },
    };
    result.map_err(|e| failed(e, "saving", account))
}

fn failed(e: Error, doing: &str, account: &str) -> anyhow::Error {
    anyhow::anyhow!("{doing} {account} in the keychain: {e}")
}
