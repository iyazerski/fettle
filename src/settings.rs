//! What Quill remembers between runs: where the model runs and its
//! credentials, the model and reasoning, the translation language and the
//! shortcut. One private JSON file, since it holds tokens.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::json_file;

pub const DEFAULT_SHORTCUT: &str = "ctrl+alt+e";
pub const DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
/// A small, fast model that ChatGPT sign-in serves.
pub const DEFAULT_CHATGPT_MODEL: &str = "gpt-6-luna";

/// The languages Translate offers, alphabetical: the 30 with the most first-
/// and second-language speakers (Ethnologue 2026), counting Chinese and Arabic
/// once each and leaving out Nigerian Pidgin, plus Belarusian, Polish and
/// Ukrainian.
pub const LANGUAGES: &[&str] = &[
    "Amharic",
    "Arabic",
    "Belarusian",
    "Bengali",
    "Chinese (Simplified)",
    "English",
    "French",
    "German",
    "Gujarati",
    "Hausa",
    "Hindi",
    "Indonesian",
    "Italian",
    "Japanese",
    "Javanese",
    "Kannada",
    "Korean",
    "Marathi",
    "Persian",
    "Polish",
    "Portuguese",
    "Punjabi",
    "Russian",
    "Spanish",
    "Swahili",
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
    /// The ChatGPT sign-in, once done.
    pub chatgpt: Option<ChatGptAuth>,
    pub chatgpt_model: String,
    /// An OpenAI-compatible API: its base URL (up to `/v1`) and key.
    pub api_base: String,
    pub api_key: String,
    pub api_model: String,
    pub reasoning: Reasoning,
    /// The language Translate writes in.
    pub language: String,
    /// Languages listed first in the picker.
    pub favourite_languages: Vec<String>,
    /// The global shortcut, like `ctrl+alt+e`.
    pub shortcut: String,
    pub appearance: Appearance,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::ChatGpt,
            chatgpt: None,
            chatgpt_model: DEFAULT_CHATGPT_MODEL.to_owned(),
            api_base: DEFAULT_API_BASE.to_owned(),
            api_key: String::new(),
            api_model: String::new(),
            reasoning: Reasoning::Off,
            language: "English".to_owned(),
            favourite_languages: Vec::new(),
            shortcut: DEFAULT_SHORTCUT.to_owned(),
            appearance: Appearance::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
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

    pub fn label(self) -> &'static str {
        match self {
            Appearance::System => "System",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }
}

/// How hard the model thinks before answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reasoning {
    /// Send nothing: the model's own default. Models without reasoning
    /// reject the parameter, so this is the safe pick for them.
    Default,
    Off,
    Low,
    Medium,
    High,
}

impl Reasoning {
    pub const ALL: [Reasoning; 5] = [
        Reasoning::Default,
        Reasoning::Off,
        Reasoning::Low,
        Reasoning::Medium,
        Reasoning::High,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Reasoning::Default => "Default",
            Reasoning::Off => "Off",
            Reasoning::Low => "Low",
            Reasoning::Medium => "Medium",
            Reasoning::High => "High",
        }
    }

    /// The API's `effort` value, or `None` to leave it out.
    pub fn effort(self) -> Option<&'static str> {
        match self {
            Reasoning::Default => None,
            Reasoning::Off => Some("none"),
            Reasoning::Low => Some("low"),
            Reasoning::Medium => Some("medium"),
            Reasoning::High => Some("high"),
        }
    }
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
    /// The settings file: `~/Library/Application Support/Quill/settings.json`.
    pub fn path() -> PathBuf {
        home().join("Library/Application Support/Quill/settings.json")
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

    /// The model for the current provider.
    pub fn model(&self) -> &str {
        match self.provider {
            Provider::ChatGpt => &self.chatgpt_model,
            Provider::Api => &self.api_model,
        }
    }

    pub fn model_mut(&mut self) -> &mut String {
        match self.provider {
            Provider::ChatGpt => &mut self.chatgpt_model,
            Provider::Api => &mut self.api_model,
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

    /// Why the model can't be called yet, if it can't.
    pub fn missing(&self) -> Option<Missing> {
        match self.provider {
            Provider::ChatGpt if self.chatgpt.is_none() => Some(Missing::SignIn),
            Provider::Api if self.api_base.trim().is_empty() => Some(Missing::ApiUrl),
            _ if self.model().trim().is_empty() => Some(Missing::Model),
            _ => None,
        }
    }
}

/// What the settings lack before the model can be called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    SignIn,
    ApiUrl,
    Model,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Missing::SignIn => "Sign in with ChatGPT in Settings",
            Missing::ApiUrl => "Set the API URL in Settings",
            Missing::Model => "Pick a model in Settings",
        })
    }
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

        let mut settings = Settings {
            provider: Provider::Api,
            api_model: "gpt-x".into(),
            reasoning: Reasoning::High,
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        assert_eq!(Settings::load(&path).model(), "gpt-x");

        // An older file without newer fields still loads.
        std::fs::write(&path, r#"{"provider":"api","api_key":"k"}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.api_key, "k");
        assert_eq!(loaded.shortcut, DEFAULT_SHORTCUT);
        settings.provider = Provider::ChatGpt;
        settings.chatgpt = None;
        assert_eq!(settings.missing(), Some(Missing::SignIn));
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
