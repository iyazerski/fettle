use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::i18n::Locale;
use crate::json_file;
use crate::llm::Effort;
use crate::prompts::Style;

/// Fettle Dev adds ⇧, so it doesn't take the installed app's shortcut.
pub const DEFAULT_SHORTCUT: &str = if crate::DEV {
    "ctrl+alt+shift+e"
} else {
    "ctrl+alt+e"
};
pub const DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
pub const DEFAULT_CHATGPT_MODEL: &str = "gpt-6-luna";

/// Ethnologue 2026's 30 most-spoken languages, excluding Nigerian Pidgin,
/// plus Belarusian, Polish, Ukrainian, and languages of major Mac markets.
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
    pub reasoning_enabled: bool,
    pub language: String,
    pub favourite_languages: Vec<String>,
    pub rewrite_style: Style,
    pub shortcut: String,
    pub appearance: Appearance,
    pub underline_mistakes: bool,
    pub menu_bar: bool,
    /// The language the window speaks; `None` follows macOS.
    pub app_language: Option<Locale>,
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
            rewrite_style: Style::Formal,
            shortcut: DEFAULT_SHORTCUT.to_owned(),
            appearance: Appearance::System,
            underline_mistakes: false,
            menu_bar: true,
            app_language: None,
            onboarded: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OnDevice,
    ChatGpt,
    Api,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Appearance::System => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Credentials {
    pub chatgpt: Option<ChatGptAuth>,
    pub api_key: String,
}

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
    pub fn path() -> PathBuf {
        home().join(format!(
            "Library/Application Support/{}/settings.json",
            crate::NAME
        ))
    }

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

    pub fn effort(&self) -> Effort {
        if self.reasoning_enabled {
            Effort::Low
        } else {
            Effort::None
        }
    }

    pub fn model(&self) -> Option<&str> {
        match self.provider {
            Provider::OnDevice => None,
            Provider::ChatGpt => Some(&self.chatgpt_model),
            Provider::Api => Some(&self.api_model),
        }
    }

    pub fn model_mut(&mut self) -> Option<&mut String> {
        match self.provider {
            Provider::OnDevice => None,
            Provider::ChatGpt => Some(&mut self.chatgpt_model),
            Provider::Api => Some(&mut self.api_model),
        }
    }

    pub fn is_favourite(&self, language: &str) -> bool {
        self.favourite_languages.iter().any(|l| l == language)
    }

    pub fn toggle_favourite(&mut self, language: &str) {
        if self.is_favourite(language) {
            self.favourite_languages.retain(|l| l != language);
        } else {
            self.favourite_languages.push(language.to_owned());
        }
    }

    pub fn languages(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        LANGUAGES
            .iter()
            .copied()
            .partition(|l| self.is_favourite(l))
    }
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME is not set"))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn settings_persist_privately_without_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        assert_eq!(Settings::load(&path), Settings::default());

        let settings = Settings {
            provider: Provider::Api,
            api_model: "gpt-x".into(),
            reasoning_enabled: true,
            rewrite_style: Style::Shorter,
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
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let mut expected = settings;
        expected.credentials = Credentials::default();
        assert_eq!(Settings::load(&path), expected);

        std::fs::write(&path, r#"{"provider":"api","api_base":"http://x"}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.api_base, "http://x");
        assert_eq!(loaded.shortcut, DEFAULT_SHORTCUT);
        assert!(loaded.menu_bar);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
