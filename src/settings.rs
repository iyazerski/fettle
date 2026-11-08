//! What Fettle remembers between runs: where the model runs, the model and
//! reasoning, the translation language, the shortcut and the window's look and
//! language, in one JSON file. The credentials live in the keychain
//! ([`keychain`](crate::keychain)), never in the file.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::i18n::Locale;
use crate::json_file;
use crate::llm::Effort;
use crate::llm::on_device::Availability;

pub const DEFAULT_SHORTCUT: &str = "ctrl+alt+e";
pub const DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
/// A small, fast model that ChatGPT sign-in serves.
pub const DEFAULT_CHATGPT_MODEL: &str = "gpt-6-luna";

/// The languages Translate offers, alphabetical: the 30 with the most first-
/// and second-language speakers (Ethnologue 2026), counting Arabic once and
/// leaving out Nigerian Pidgin, plus Belarusian, Polish and Ukrainian, and the
/// languages of wealthy markets where Macs are common: Traditional Chinese,
/// Danish, Dutch, Finnish, Hebrew, Norwegian and Swedish.
pub const LANGUAGES: [&str; 40] = [
    "Amharic",
    "Arabic",
    "Belarusian",
    "Bengali",
    "Chinese (Simplified)",
    "Chinese (Traditional)",
    "Danish",
    "Dutch",
    "English",
    "Finnish",
    "French",
    "German",
    "Gujarati",
    "Hausa",
    "Hebrew",
    "Hindi",
    "Indonesian",
    "Italian",
    "Japanese",
    "Javanese",
    "Kannada",
    "Korean",
    "Marathi",
    "Norwegian",
    "Persian",
    "Polish",
    "Portuguese",
    "Punjabi",
    "Russian",
    "Spanish",
    "Swahili",
    "Swedish",
    "Tagalog",
    "Tamil",
    "Telugu",
    "Thai",
    "Turkish",
    "Ukrainian",
    "Urdu",
    "Vietnamese",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: Provider,
    /// Kept in the keychain.
    #[serde(skip)]
    pub credentials: Credentials,
    pub chatgpt_model: String,
    /// An OpenAI-compatible API's base URL, up to `/v1`.
    pub api_base: String,
    pub api_model: String,
    /// Whether the model thinks a little before it answers: slower, and
    /// rarely needed for these tasks.
    pub reasoning_enabled: bool,
    /// The language Translate writes in.
    pub language: String,
    /// Languages listed first in the picker.
    pub favourite_languages: Vec<String>,
    /// The global shortcut, like `ctrl+alt+e`.
    pub shortcut: String,
    pub appearance: Appearance,
    /// Whether mistakes in other apps' text fields get underlined as the user types.
    pub underline_mistakes: bool,
    /// The language the window speaks; `None` follows macOS.
    pub app_language: Option<Locale>,
    /// Whether the welcome steps are done.
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::OnDevice,
            credentials: Credentials::default(),
            chatgpt_model: DEFAULT_CHATGPT_MODEL.to_owned(),
            api_base: DEFAULT_API_BASE.to_owned(),
            api_model: String::new(),
            reasoning_enabled: false,
            language: "English".to_owned(),
            favourite_languages: Vec::new(),
            shortcut: DEFAULT_SHORTCUT.to_owned(),
            appearance: Appearance::System,
            underline_mistakes: false,
            app_language: None,
            onboarded: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Apple's on-device model.
    OnDevice,
    /// A ChatGPT plan, through the sign-in Codex uses.
    ChatGpt,
    /// Any OpenAI-compatible chat completions API.
    Api,
}

/// Light or dark window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// Follow macOS.
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    /// Whether this draws dark when macOS draws dark as `system_dark` says.
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Appearance::System => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }
}

/// The secrets in the settings, which the keychain keeps rather than the file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Credentials {
    /// The ChatGPT sign-in, once done.
    pub chatgpt: Option<ChatGptAuth>,
    /// The OpenAI-compatible API's key.
    pub api_key: String,
}

/// The tokens from signing in with ChatGPT.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatGptAuth {
    pub access: String,
    pub refresh: String,
    /// When `access` expires, in Unix seconds.
    pub expires_at: i64,
    pub account_id: String,
    pub email: Option<String>,
}

impl Settings {
    /// The settings file: `~/Library/Application Support/Fettle/settings.json`.
    pub fn path() -> PathBuf {
        home().join("Library/Application Support/Fettle/settings.json")
    }

    /// Load the settings at `path`; a missing file gives the defaults, and a
    /// broken one is reported and replaced by them.
    pub fn load(path: &Path) -> Self {
        json_file::load::<Settings>(path)
            .unwrap_or_else(|e| {
                eprintln!("{e:#}");
                None
            })
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        json_file::save(path, self)
    }

    /// Return the reasoning effort to ask for: a little when reasoning is on, none otherwise.
    pub fn effort(&self) -> Effort {
        if self.reasoning_enabled {
            Effort::Low
        } else {
            Effort::None
        }
    }

    /// Return the model for the current provider; `None` on device, which has only one.
    pub fn model(&self) -> Option<&str> {
        match self.provider {
            Provider::OnDevice => None,
            Provider::ChatGpt => Some(&self.chatgpt_model),
            Provider::Api => Some(&self.api_model),
        }
    }

    /// Return the current provider's model to change, unless it has only one.
    pub fn model_mut(&mut self) -> Option<&mut String> {
        match self.provider {
            Provider::OnDevice => None,
            Provider::ChatGpt => Some(&mut self.chatgpt_model),
            Provider::Api => Some(&mut self.api_model),
        }
    }

    /// Whether `language` is a favourite.
    pub fn is_favourite(&self, language: &str) -> bool {
        self.favourite_languages.iter().any(|l| l == language)
    }

    /// Add `language` to the favourites, or take it out.
    pub fn toggle_favourite(&mut self, language: &str) {
        if self.is_favourite(language) {
            self.favourite_languages.retain(|l| l != language);
        } else {
            self.favourite_languages.push(language.to_owned());
        }
    }

    /// [`LANGUAGES`] split into the favourites and the rest, both alphabetical.
    pub fn languages(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        LANGUAGES
            .iter()
            .copied()
            .partition(|l| self.is_favourite(l))
    }
}

/// What the settings lack before the model can be called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    OnDevice(Availability),
    SignIn,
    ApiUrl,
    Model,
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_restart_and_fill_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        assert_eq!(Settings::load(&path), Settings::default());

        let settings = Settings {
            provider: Provider::Api,
            api_model: "gpt-x".into(),
            reasoning_enabled: true,
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        assert_eq!(Settings::load(&path).model(), Some("gpt-x"));

        // An older file, without newer fields and with ones since dropped, still loads.
        std::fs::write(
            &path,
            r#"{"provider":"api","api_base":"http://x","reasoning":"high"}"#,
        )
        .unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.api_base, "http://x");
        assert_eq!(loaded.shortcut, DEFAULT_SHORTCUT);
    }

    #[test]
    fn credentials_stay_out_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings {
            credentials: Credentials {
                api_key: "sk-secret".into(),
                chatgpt: Some(ChatGptAuth {
                    access: "access-secret".into(),
                    refresh: "refresh-secret".into(),
                    expires_at: 0,
                    account_id: "acct".into(),
                    email: None,
                }),
            },
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        let file = std::fs::read_to_string(&path).unwrap();
        assert!(!file.contains("secret"), "{file}");
        let loaded = Settings::load(&path);
        assert_eq!(loaded.credentials, Credentials::default());
    }

    #[test]
    fn favourite_languages_come_first() {
        let mut settings = Settings::default();
        settings.toggle_favourite("Spanish");
        settings.toggle_favourite("Arabic");
        settings.toggle_favourite("German");
        settings.toggle_favourite("Spanish");
        let (favourites, rest) = settings.languages();
        assert_eq!(favourites, ["Arabic", "German"]);
        assert_eq!(rest.len(), LANGUAGES.len() - 2);
        assert!(rest.contains(&"Spanish") && !rest.contains(&"German"));
    }
}
