//! The languages Fettle's own window speaks, and its text in each. Prompts to
//! the model stay in English; only what the user reads changes.

mod ar;
mod de;
mod en;
mod es;
mod fr;
mod hi;
mod ja;
mod ko;
mod pl;
mod pt;
mod ru;
mod zh;

use serde::{Deserialize, Serialize};

use crate::dictation::Problem;
use crate::llm::Missing;
use crate::llm::on_device::{Availability, Refusal};
use crate::prompts::Style;
use crate::settings::LANGUAGES;

/// A language the window can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    English,
    Spanish,
    Chinese,
    Hindi,
    French,
    Russian,
    Arabic,
    Portuguese,
    German,
    Japanese,
    Polish,
    Korean,
}

impl Locale {
    pub const ALL: [Locale; 12] = [
        Locale::English,
        Locale::Spanish,
        Locale::Chinese,
        Locale::Hindi,
        Locale::French,
        Locale::Russian,
        Locale::Arabic,
        Locale::Portuguese,
        Locale::German,
        Locale::Japanese,
        Locale::Polish,
        Locale::Korean,
    ];

    /// Return the language's name in itself, as the picker lists it.
    pub fn native_name(self) -> &'static str {
        match self {
            Locale::English => "English",
            Locale::Spanish => "Español",
            Locale::Chinese => "中文",
            Locale::Hindi => "हिन्दी",
            Locale::French => "Français",
            Locale::Russian => "Русский",
            Locale::Arabic => "العربية",
            Locale::Portuguese => "Português",
            Locale::German => "Deutsch",
            Locale::Japanese => "日本語",
            Locale::Polish => "Polski",
            Locale::Korean => "한국어",
        }
    }

    /// Return the first of `preferred` (BCP 47 tags like `pt-BR`, most
    /// preferred first) that Fettle speaks, or English.
    pub fn from_preferred<S: AsRef<str>>(preferred: &[S]) -> Locale {
        preferred
            .iter()
            .find_map(|tag| {
                let code = tag.as_ref().split(['-', '_']).next().unwrap_or_default();
                Some(match code {
                    "en" => Locale::English,
                    "es" => Locale::Spanish,
                    "zh" => Locale::Chinese,
                    "hi" => Locale::Hindi,
                    "fr" => Locale::French,
                    "ru" => Locale::Russian,
                    "ar" => Locale::Arabic,
                    "pt" => Locale::Portuguese,
                    "de" => Locale::German,
                    "ja" => Locale::Japanese,
                    "pl" => Locale::Polish,
                    "ko" => Locale::Korean,
                    _ => return None,
                })
            })
            .unwrap_or(Locale::English)
    }

    /// Return the window's text in this language.
    pub fn strings(self) -> &'static Strings {
        match self {
            Locale::English => &en::STRINGS,
            Locale::Spanish => &es::STRINGS,
            Locale::Chinese => &zh::STRINGS,
            Locale::Hindi => &hi::STRINGS,
            Locale::French => &fr::STRINGS,
            Locale::Russian => &ru::STRINGS,
            Locale::Arabic => &ar::STRINGS,
            Locale::Portuguese => &pt::STRINGS,
            Locale::German => &de::STRINGS,
            Locale::Japanese => &ja::STRINGS,
            Locale::Polish => &pl::STRINGS,
            Locale::Korean => &ko::STRINGS,
        }
    }
}

/// Everything the window says, in one language. A `{}` marks where a value
/// goes; fill it with [`fill`].
pub struct Strings {
    // The main window.
    pub text: &'static str,
    pub placeholder: &'static str,
    pub clear: &'static str,
    pub dictate: &'static str,
    pub stop_dictating: &'static str,
    pub speech_off: &'static str,
    pub microphone_off: &'static str,
    pub speech_unavailable: &'static str,
    /// `{}`: macOS's description of the error.
    pub dictation_failed: &'static str,
    pub back: &'static str,
    pub settings: &'static str,
    pub pin: &'static str,
    pub unpin: &'static str,
    pub hide: &'static str,
    pub fix: &'static str,
    pub stop: &'static str,
    pub translate: &'static str,
    pub translation_language: &'static str,
    pub rewrite_style: &'static str,
    pub add_favourite: &'static str,
    pub remove_favourite: &'static str,
    pub fixing: &'static str,
    pub fixed: &'static str,
    /// `{}`: the language.
    pub translating: &'static str,
    /// `{}`: the language.
    pub translation: &'static str,
    pub rewrite: &'static str,
    pub rewriting: &'static str,
    /// `{}`: the style.
    pub rewritten: &'static str,
    /// Formal, Friendly, Shorter, Clearer, as [`Style::ALL`](crate::prompts::Style::ALL).
    pub styles: [&'static str; 4],
    /// When to pick each of `styles`, in the same order, shown under its name.
    pub style_hints: [&'static str; 4],
    pub show_changes: &'static str,
    pub thinking: &'static str,
    pub nothing_to_fix: &'static str,
    pub copy: &'static str,
    /// With `copy`, the items of a text field's menu.
    pub cut: &'static str,
    pub paste: &'static str,
    pub copied: &'static str,
    pub replace: &'static str,
    pub replace_help: &'static str,
    pub no_selection: &'static str,
    pub no_text: &'static str,
    /// Replace found the clicked text gone from where it was.
    pub text_moved: &'static str,
    pub stopped: &'static str,
    pub empty_answer: &'static str,
    /// `{}`: the error.
    pub save_failed: &'static str,
    pub missing_sign_in: &'static str,
    pub missing_api_url: &'static str,
    pub missing_model: &'static str,
    // Settings.
    pub model: &'static str,
    pub chatgpt_plan: &'static str,
    pub compatible_api: &'static str,
    pub on_device: &'static str,
    pub on_device_ready: &'static str,
    pub on_device_old_macos: &'static str,
    pub on_device_not_eligible: &'static str,
    pub on_device_turned_off: &'static str,
    pub on_device_not_ready: &'static str,
    /// Opens Apple Intelligence in System Settings.
    pub open_system_settings: &'static str,
    pub too_long: &'static str,
    pub unsupported_language: &'static str,
    pub declined: &'static str,
    pub welcome_title: &'static str,
    /// `{}`: the shortcut.
    pub welcome_body: &'static str,
    pub get_started: &'static str,
    pub next: &'static str,
    pub model_title: &'static str,
    // The choices in the welcome steps, under their names.
    pub on_device_body: &'static str,
    pub chatgpt_body: &'static str,
    pub api_body: &'static str,
    pub access_title: &'static str,
    pub access_body: &'static str,
    pub done_title: &'static str,
    /// `{}`: the shortcut.
    pub done_body: &'static str,
    pub try_it: &'static str,
    pub done: &'static str,
    /// A sentence with mistakes for Fix to correct.
    pub sample: &'static str,
    pub reasoning: &'static str,
    /// What turning reasoning on does, under its switch.
    pub reasoning_hint: &'static str,
    pub general: &'static str,
    pub appearance: &'static str,
    /// System, Light, Dark.
    pub appearances: [&'static str; 3],
    pub app_language: &'static str,
    /// The language picker's choice to follow macOS.
    pub system_language: &'static str,
    pub shortcut: &'static str,
    pub open_fettle: &'static str,
    pub underline: &'static str,
    /// What underlining does, under its switch.
    pub underline_hint: &'static str,
    pub start_at_login: &'static str,
    pub menu_bar: &'static str,
    pub accessibility: &'static str,
    pub allowed: &'static str,
    pub allow: &'static str,
    pub allow_help: &'static str,
    pub check_again: &'static str,
    pub accessibility_needed: &'static str,
    pub made_by: &'static str,
    pub buy_coffee: &'static str,
    pub mit_license: &'static str,
    pub notices: &'static str,
    pub check_updates: &'static str,
    pub checking: &'static str,
    pub up_to_date: &'static str,
    /// `{}`: the version.
    pub update_to: &'static str,
    pub update_help: &'static str,
    /// `{}`: the version.
    pub updating: &'static str,
    pub quit: &'static str,
    pub finish_sign_in: &'static str,
    pub cancel: &'static str,
    /// `{}`: the email.
    pub signed_in_as: &'static str,
    pub chatgpt_user: &'static str,
    pub sign_out: &'static str,
    pub sign_in: &'static str,
    pub api_url: &'static str,
    pub api_key: &'static str,
    pub api_key_placeholder: &'static str,
    pub loading_models: &'static str,
    pub choose_model: &'static str,
    pub try_again: &'static str,
    pub no_models: &'static str,
    /// `{}`: the error.
    pub list_models_failed: &'static str,
    /// `{}`: the error.
    pub shortcut_broken: &'static str,
    /// `{}`: the app that has the shortcut.
    pub shortcut_taken: &'static str,
    /// `{}`: the error.
    pub shortcut_failed: &'static str,
    /// `{}`: the error.
    pub check_failed: &'static str,
    /// `{}`: the error.
    pub update_failed: &'static str,
    /// The names of [`LANGUAGES`], in its order.
    pub languages: [&'static str; LANGUAGES.len()],
}

impl Strings {
    /// Return what to do about `missing`.
    pub fn missing(&self, missing: Missing) -> &'static str {
        match missing {
            Missing::OnDevice(availability) => self.on_device_status(availability),
            Missing::SignIn => self.missing_sign_in,
            Missing::ApiUrl => self.missing_api_url,
            Missing::Model => self.missing_model,
        }
    }

    /// Return what `availability` means for the on-device model, in this language.
    pub fn on_device_status(&self, availability: Availability) -> &'static str {
        match availability {
            Availability::Ready => self.on_device_ready,
            Availability::OldMacOS => self.on_device_old_macos,
            Availability::NotEligible => self.on_device_not_eligible,
            Availability::TurnedOff => self.on_device_turned_off,
            Availability::NotReady => self.on_device_not_ready,
        }
    }

    /// Return why the on-device model gave no answer, in this language.
    pub fn refusal(&self, refusal: Refusal) -> &'static str {
        match refusal {
            Refusal::TooLong => self.too_long,
            Refusal::Language => self.unsupported_language,
            Refusal::Declined => self.declined,
        }
    }

    /// Return why dictation can't run, in this language.
    pub fn dictation_problem(&self, problem: &Problem) -> String {
        match problem {
            Problem::SpeechOff => self.speech_off.into(),
            Problem::MicrophoneOff => self.microphone_off.into(),
            Problem::Unavailable => self.speech_unavailable.into(),
            Problem::Failed(why) => fill(self.dictation_failed, why),
        }
    }

    /// Return the name of `style` in this language.
    pub fn style(&self, style: Style) -> &'static str {
        self.styles[style as usize]
    }

    /// Return when to pick `style`, in this language.
    pub fn style_hint(&self, style: Style) -> &'static str {
        self.style_hints[style as usize]
    }

    /// Return the name of `language`, one of [`LANGUAGES`], in this language.
    pub fn language(&self, language: &str) -> &'static str {
        LANGUAGES
            .iter()
            .position(|l| *l == language)
            .map_or("?", |i| self.languages[i])
    }

    /// Sort `languages`, from [`LANGUAGES`], by their names in this language.
    pub fn sort_languages(&self, languages: &mut [&'static str]) {
        languages.sort_by_cached_key(|l| sort_key(self.language(l)));
    }
}

/// Put `value` in place of the `{}` in `template`.
pub fn fill(template: &str, value: impl std::fmt::Display) -> String {
    template.replacen("{}", &value.to_string(), 1)
}

/// `name` lowercased with Latin accents dropped, so "Árabe" sorts with the a's.
fn sort_key(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'î' | 'ï' => 'i',
            'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' | 'ń' => 'n',
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_first_preferred_language_fettle_speaks() {
        let cases: &[(&[&str], Locale)] = &[
            (&["pt-BR", "en-US"], Locale::Portuguese),
            (&["uk-UA", "ru-RU", "en"], Locale::Russian),
            (&["zh-Hans-CN"], Locale::Chinese),
            (&["ko-KR"], Locale::Korean),
            (&["it-IT", "nl-NL"], Locale::English),
            (&[], Locale::English),
        ];
        for (preferred, want) in cases {
            assert_eq!(Locale::from_preferred(preferred), *want, "{preferred:?}");
        }
    }

    #[test]
    fn sorts_translation_languages_by_their_local_names() {
        let mut languages = vec!["Vietnamese", "Arabic", "German", "Amharic"];
        Locale::Spanish.strings().sort_languages(&mut languages);
        // Alemán, Amárico, Árabe, Vietnamita.
        assert_eq!(languages, ["German", "Amharic", "Arabic", "Vietnamese"]);
    }
}
