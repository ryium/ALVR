//! Dashboard-local preferences and English-first UI translations.
use eframe::egui::{Context, Id, Ui};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::BTreeMap, path::Path, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ja")]
    Japanese,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    language: Language,
}

fn catalog() -> &'static BTreeMap<String, String> {
    static CATALOG: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../locales/ja.json"))
            .expect("Invalid Japanese UI catalog")
    })
}

impl Language {
    pub fn load_local() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self::load(
                &crate::get_filesystem_layout()
                    .config_dir
                    .join("frameconnect-ui.json"),
            )
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::default()
        }
    }

    pub fn save_local(self) -> std::io::Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.save(
                &crate::get_filesystem_layout()
                    .config_dir
                    .join("frameconnect-ui.json"),
            )
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(())
        }
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
            .unwrap_or_default()
            .language
    }

    pub fn save(self, path: &Path) -> std::io::Result<()> {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&Preferences { language: self })?,
        )
    }

    pub fn set_context(self, ctx: &Context) {
        ctx.data_mut(|data| data.insert_temp(Id::new("frameconnect.language"), self));
    }

    pub fn from_context(ctx: &Context) -> Self {
        ctx.data(|data| data.get_temp::<Self>(Id::new("frameconnect.language")))
            .unwrap_or_default()
    }

    pub fn text<'a>(self, english: &'a str) -> &'a str {
        match self {
            Self::English => english,
            Self::Japanese => catalog().get(english).map_or(english, String::as_str),
        }
    }
}

pub fn tr<'a>(ui: &Ui, english: &'a str) -> &'a str {
    Language::from_context(ui.ctx()).text(english)
}

pub fn message<'a>(ui: &Ui, english: &'a str) -> Cow<'a, str> {
    translated_message(Language::from_context(ui.ctx()), english)
}

fn translated_message(language: Language, english: &str) -> Cow<'_, str> {
    if language == Language::Japanese {
        if let Some(values) = english.strip_prefix("Streaming at ").and_then(|s| {
            s.strip_suffix(" Hz target. Lowering the refresh rate while preserving quality.")
        }) && let Some((fps, hz)) = values.split_once(" fps is below the ")
        {
            return language.text("Streaming at {fps} fps is below the {hz} Hz target. Lowering the refresh rate while preserving quality.")
                .replace("{fps}", fps).replace("{hz}", hz).into();
        }
        if let Some(error) = english.strip_prefix("Could not verify the device hardware decoder: ")
        {
            return format!(
                "{}: {error}",
                language.text("Could not verify the device hardware decoder")
            )
            .into();
        }
    }
    Cow::Borrowed(language.text(english))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_default_and_unknown_messages_are_preserved() {
        assert_eq!(Language::default(), Language::English);
        assert_eq!(Language::English.text("Settings"), "Settings");
        assert_eq!(Language::Japanese.text("Settings"), "設定");
        assert_eq!(
            Language::Japanese.text("Device-specific error 42"),
            "Device-specific error 42"
        );
    }

    #[test]
    fn dynamic_messages_preserve_measurements_and_diagnostic_details() {
        let original = "Streaming at 89.2 fps is below the 120 Hz target. Lowering the refresh rate while preserving quality.";
        assert_eq!(translated_message(Language::English, original), original);
        let japanese = translated_message(Language::Japanese, original);
        assert!(japanese.contains("89.2 fps"));
        assert!(japanese.contains("120 Hz"));
        assert!(japanese.contains("画質"));
        let error = "Could not verify the device hardware decoder: /dev/video-dec0: EINVAL";
        assert!(translated_message(Language::Japanese, error).ends_with("/dev/video-dec0: EINVAL"));
    }

    #[test]
    fn context_switches_both_ways_without_restart() {
        let ctx = Context::default();
        for language in [Language::English, Language::Japanese, Language::English] {
            language.set_context(&ctx);
            let _ = ctx.run_ui(Default::default(), |ui| {
                eframe::egui::CentralPanel::default().show(ui, |ui| {
                    assert_eq!(tr(ui, "Devices"), language.text("Devices"));
                });
            });
        }
    }

    #[test]
    fn preference_survives_restart_and_invalid_files_fall_back_to_english() {
        let path = std::env::temp_dir().join(format!(
            "frameconnect-language-{}-{}.json",
            std::process::id(),
            rand::random::<u64>()
        ));
        assert_eq!(Language::load(&path), Language::English);
        Language::Japanese.save(&path).unwrap();
        assert_eq!(Language::load(&path), Language::Japanese);
        Language::English.save(&path).unwrap();
        assert_eq!(Language::load(&path), Language::English);
        for invalid in [r#"{}"#, r#"{"language":"unknown"}"#, "invalid JSON"] {
            std::fs::write(&path, invalid).unwrap();
            assert_eq!(Language::load(&path), Language::English);
        }
        std::fs::remove_file(path).unwrap();
    }
}
