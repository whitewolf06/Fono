use tauri::State;

use crate::error::AppResult;
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
