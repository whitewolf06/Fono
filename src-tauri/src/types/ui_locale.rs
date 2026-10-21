//! UI locale is independent of recognition and translation languages.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    Ru,
    En,
    #[default]
    #[serde(other)]
    Auto,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiLocale {
    Ru,
    En,
}
impl UiLanguage {
    pub fn resolve_for(self, system: UiLocale) -> UiLocale {
        match self {
            Self::Ru => UiLocale::Ru,
            Self::En => UiLocale::En,
            Self::Auto => system,
        }
    }
    pub fn resolve(self) -> UiLocale {
        self.resolve_for(system_ui_locale())
    }
}
pub fn system_ui_locale() -> UiLocale {
    #[cfg(windows)]
    // This reads the user's Windows UI language, with no device or disk I/O.
    let russian =
        unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() } & 0x03ff == 0x19;
    #[cfg(not(windows))]
    let russian = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|s| !s.is_empty()))
        .is_some_and(|s| s.to_ascii_lowercase().starts_with("ru"));
    if russian {
        UiLocale::Ru
    } else {
        UiLocale::En
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_migration_and_fallback_are_separate_from_speech_language() {
        let settings: super::super::Settings =
            serde_json::from_str(r#"{"language":"en"}"#).unwrap();
        assert_eq!(settings.ui_language, UiLanguage::Auto);
        for system in [UiLocale::Ru, UiLocale::En] {
            assert_eq!(UiLanguage::Auto.resolve_for(system), system);
            assert_eq!(UiLanguage::Ru.resolve_for(system), UiLocale::Ru);
            assert_eq!(UiLanguage::En.resolve_for(system), UiLocale::En);
        }
        let unknown: UiLanguage = serde_json::from_str(r#""future-locale""#).unwrap();
        assert_eq!(unknown, UiLanguage::Auto);
        let restored: super::super::Settings = serde_json::from_value(
            serde_json::to_value(super::super::Settings {
                ui_language: UiLanguage::En,
                language: "ru".into(),
                ..settings
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(restored.ui_language, UiLanguage::En);
        assert_eq!(restored.language, "ru");
    }
}
