use tauri::State;

use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::state::AppState;

#[tauri::command]
pub async fn test_llm_connection(state: State<'_, AppState>) -> AppResult<String> {
    let settings = state.settings();
    let model_id = LlmClient::from_settings(&settings)
        .test_connection()
        .await?;
    Ok(format!("LLM active, model: {model_id}"))
}

#[tauri::command]
pub async fn list_llm_models(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let settings = state.settings();
    LlmClient::from_settings(&settings).list_models().await
}

#[tauri::command]
pub async fn test_llm_profile(state: State<'_, AppState>, profile_id: String) -> AppResult<String> {
    let settings = state.settings();
    let profile = settings
        .llm_profile(Some(&profile_id))
        .ok_or_else(|| AppError::Config("LLM-профиль не найден".into()))?;
    let model_id = LlmClient::from_profile(profile, None)
        .test_connection()
        .await?;
    Ok(format!(
        "Профиль «{}» доступен, модель: {model_id}",
        profile.name
    ))
}

#[tauri::command]
pub async fn list_llm_profile_models(
    state: State<'_, AppState>,
    profile_id: String,
) -> AppResult<Vec<String>> {
    let settings = state.settings();
    let profile = settings
        .llm_profile(Some(&profile_id))
        .ok_or_else(|| AppError::Config("LLM-профиль не найден".into()))?;
    LlmClient::from_profile(profile, None).list_models().await
}
