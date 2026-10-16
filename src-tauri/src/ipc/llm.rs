use tauri::State;

use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::state::AppState;

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
    profile_id: String,
) -> AppResult<Vec<String>> {
    let _activity = crate::application::updates::activity::lease()?;
    let settings = state.settings();
    let profile = settings
        .llm_profile(Some(&profile_id))
        .cloned()
        .ok_or_else(|| AppError::Config("LLM-профиль не найден".into()))?;
    let client = super::blocking::run(move || Ok(LlmClient::from_profile(&profile, None))).await?;
    client.list_models().await
}
