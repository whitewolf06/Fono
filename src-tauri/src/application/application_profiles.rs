//! Local configuration and per-operation overrides; never retain process paths.
use crate::{
    error::{AppError, AppResult},
    types::{AiMode, InjectionMode, Settings, TextPreset, TranslationLanguage},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationProfile {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub executables: Vec<String>,
    pub processing_enabled: Option<bool>,
    pub preset: Option<TextPreset>,
    pub translation_enabled: Option<bool>,
    pub target_language: Option<TranslationLanguage>,
    pub language: Option<String>,
    pub injection_mode: Option<InjectionMode>,
}

pub fn validate(profiles: &[ApplicationProfile]) -> AppResult<()> {
    let mut ids = HashSet::new();
    let mut executables = HashSet::new();
    let invalid = profiles.len() > 32
        || profiles.iter().any(|p| {
            p.id.is_empty()
                || p.id.len() > 64
                || !p
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                || !ids.insert(p.id.as_str())
                || p.name.trim().is_empty()
                || p.name.chars().count() > 64
                || p.name.chars().any(char::is_control)
                || p.executables.is_empty()
                || p.executables.len() > 16
                || p.executables.iter().any(|exe| {
                    !valid_executable(exe) || !executables.insert(exe.to_ascii_lowercase())
                })
                || p.language
                    .as_ref()
                    .is_some_and(|s| !matches!(s.as_str(), "auto" | "ru" | "en"))
        });
    if invalid {
        return Err(AppError::Config("Проверьте профили приложений: нужны уникальные имена exe без путей, название до 64 символов и допустимый язык".into()));
    }
    Ok(())
}

fn valid_executable(exe: &str) -> bool {
    exe.len() > 4
        && exe.len() <= 128
        && exe.trim() == exe
        && exe.to_ascii_lowercase().ends_with(".exe")
        && exe
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" ._-".contains(&b))
}

/// The caller captures executable identity once, tied to the insertion target.
pub fn resolve(mut settings: Settings, executable: Option<&str>) -> Settings {
    settings.active_application_profile = None;
    if validate(&settings.application_profiles).is_err() {
        return settings;
    }
    let Some(profile) = executable
        .and_then(|exe| {
            settings.application_profiles.iter().find(|p| {
                p.enabled
                    && p.executables
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(exe))
            })
        })
        .cloned()
    else {
        return settings;
    };
    if let Some(enabled) = profile.processing_enabled {
        settings.ai_mode = if enabled { AiMode::Clean } else { AiMode::Off };
    }
    if let Some(preset) = profile.preset {
        settings.processing_preset = Some(preset);
    }
    if let Some(enabled) = profile.translation_enabled {
        settings.processing_translation_enabled = enabled;
    }
    if let Some(language) = profile.target_language {
        settings.processing_target_language = Some(language);
    }
    if let Some(language) = profile.language {
        settings.language = language;
    }
    if let Some(mode) = profile.injection_mode {
        settings.injection_mode = mode;
    }
    settings.active_application_profile = Some(profile.name.trim().to_owned());
    settings
}

/// Only the display label and confirmed choice leave the native snapshot.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPreferences {
    pub session_id: u64,
    pub profile_name: Option<String>,
    pub preset: TextPreset,
    pub target_language: Option<TranslationLanguage>,
    pub processing_enabled: bool,
    pub translation_enabled: bool,
}
impl SessionPreferences {
    pub fn new(session_id: u64, settings: &Settings) -> Self {
        Self {
            session_id,
            profile_name: settings.active_application_profile.clone(),
            preset: crate::application::dictation::workflow::effective_preset(settings),
            target_language: settings.processing_target_language,
            processing_enabled: settings.ai_mode != AiMode::Off,
            translation_enabled: settings.processing_translation_enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(executable: &str) -> ApplicationProfile {
        ApplicationProfile {
            id: "ide".into(),
            name: "IDE".into(),
            enabled: true,
            executables: vec![executable.into()],
            processing_enabled: Some(true),
            preset: Some(TextPreset::Task),
            translation_enabled: Some(false),
            target_language: None,
            language: Some("en".into()),
            injection_mode: Some(InjectionMode::SendInput),
        }
    }
    #[test]
    fn profile_changes_only_session_options_and_unknown_or_disabled_uses_global() {
        let mut base = Settings {
            application_profiles: vec![profile("code.exe")],
            ..Settings::default()
        };
        let selected = resolve(base.clone(), Some("Code.EXE"));
        assert_eq!(selected.processing_preset, Some(TextPreset::Task));
        assert_eq!(selected.language, "en");
        assert_eq!(selected.injection_mode, InjectionMode::SendInput);
        assert!(!selected.processing_translation_enabled);
        assert_eq!(selected.active_application_profile.as_deref(), Some("IDE"));
        assert_eq!(base.processing_preset, None);
        for exe in [None, Some("terminal.exe")] {
            assert_eq!(
                serde_json::to_value(resolve(base.clone(), exe)).unwrap(),
                serde_json::to_value(&base).unwrap()
            );
        }
        base.application_profiles[0].enabled = false;
        assert!(resolve(base, Some("code.exe"))
            .active_application_profile
            .is_none());
    }
    #[test]
    fn invalid_or_ambiguous_profiles_fail_closed_without_echoing_private_values() {
        let mut profiles = vec![profile("code.exe")];
        for exe in [
            "C:\\PRIVATE\\code.exe",
            "../code.exe",
            "code.exe\n",
            "*.exe",
        ] {
            profiles[0].executables = vec![exe.into()];
            let error = validate(&profiles).unwrap_err().to_string();
            assert!(!error.contains("PRIVATE"));
        }
        profiles = vec![profile("code.exe"), profile("CODE.EXE")];
        profiles[1].id = "other".into();
        assert!(validate(&profiles).is_err());
        assert!(resolve(
            Settings {
                application_profiles: profiles,
                ..Settings::default()
            },
            Some("code.exe")
        )
        .active_application_profile
        .is_none());
    }
    #[test]
    fn persisted_settings_never_contain_transient_identity_or_applied_label() {
        let base = Settings {
            application_profiles: vec![profile("code.exe")],
            ..Settings::default()
        };
        let selected = resolve(base, Some("code.exe"));
        let json = serde_json::to_value(&selected).unwrap();
        assert!(json.get("active_application_profile").is_none());
        let resumed: Settings = serde_json::from_value(json).unwrap();
        assert!(resumed.active_application_profile.is_none());
        let snapshot = serde_json::to_value(SessionPreferences::new(7, &selected)).unwrap();
        assert_eq!(snapshot["profileName"], "IDE");
        assert!(!snapshot.to_string().contains("code.exe"));
        assert!(snapshot.get("path").is_none());
    }
}
