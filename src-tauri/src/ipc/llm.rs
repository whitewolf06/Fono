use tauri::State;

use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalLlmPreset {
    Ollama,
    Lmstudio,
}
impl LocalLlmPreset {
    fn endpoint(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434/v1",
            Self::Lmstudio => "http://127.0.0.1:1234/v1",
        }
    }
    fn client(self) -> LlmClient {
        // A draft preset never opens the credentials vault or supplies a key.
        LlmClient::new(self.endpoint(), None, None)
    }
}

#[tauri::command]
pub async fn test_llm_profile(state: State<'_, AppState>, profile_id: String) -> AppResult<String> {
    let _activity = crate::application::updates::activity::lease()?;
    let settings = state.settings();
    let profile = settings
        .llm_profile(Some(&profile_id))
        .cloned()
        .ok_or_else(|| AppError::Config("LLM-профиль не найден".into()))?;
    let name = profile.name.clone();
    let client = super::blocking::run(move || Ok(LlmClient::from_profile(&profile, None))).await?;
    let model_id = client.test_connection().await?;
    Ok(format!("Профиль «{}» доступен, модель: {model_id}", name))
}

#[tauri::command]
pub async fn list_llm_profile_models(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    local_preset: Option<LocalLlmPreset>,
) -> AppResult<Vec<String>> {
    let _activity = crate::application::updates::activity::lease()?;
    let client = match (profile_id, local_preset) {
        (None, Some(preset)) => preset.client(),
        (Some(profile_id), None) => {
            let profile = state
                .settings()
                .llm_profile(Some(&profile_id))
                .cloned()
                .ok_or_else(|| AppError::Config("LLM-профиль не найден".into()))?;
            super::blocking::run(move || Ok(LlmClient::from_profile(&profile, None))).await?
        }
        _ => {
            return Err(AppError::Config(
                "Выберите один профиль или локальный шаблон.".into(),
            ))
        }
    };
    client.list_models().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_discovery_accepts_only_two_builtin_loopback_targets() {
        for (json, expected) in [
            ("\"ollama\"", "http://127.0.0.1:11434/v1"),
            ("\"lmstudio\"", "http://127.0.0.1:1234/v1"),
        ] {
            let preset: LocalLlmPreset = serde_json::from_str(json).unwrap();
            assert_eq!(preset.endpoint(), expected);
            let _ = preset.client(); // Construction is memory-only, no network/vault.
        }
        for json in ["\"custom\"", "\"http://example.test\"", "\"../ollama\""] {
            assert!(serde_json::from_str::<LocalLlmPreset>(json).is_err());
        }
    }
}
