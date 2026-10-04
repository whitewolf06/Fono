//! Desktop presentation contracts. Session text and microphone samples stay in memory.
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
    types::*,
};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
pub struct DesktopSession {
    pub last: Mutex<Option<LastDictation>>,
    pub microphone_sample: Mutex<Vec<i16>>,
}
#[derive(Clone, Serialize)]
pub struct LastDictation {
    pub id: String,
    pub text: String,
    pub original_text: String,
    pub created_at: String,
    pub audio_secs: f64,
}
#[derive(Serialize)]
pub struct DesktopSnapshot {
    state: PipelineState,
    level: f32,
    last: Option<LastDictation>,
    operation_id: u64,
    source: Option<crate::operation::OperationSource>,
}
#[tauri::command]
pub fn get_desktop_snapshot(app: AppHandle) -> DesktopSnapshot {
    let pipeline = app.state::<Pipeline>();
    DesktopSnapshot {
        state: app.state::<AppState>().pipeline_state(),
        level: pipeline.current_level().clamp(0.0, 1.0),
        last: app.state::<DesktopSession>().last.lock().clone(),
        operation_id: pipeline.operation_id(),
        source: pipeline.current_operation().map(|o| o.source),
    }
}
pub fn publish_result(app: &AppHandle, result: LastDictation) {
    *app.state::<DesktopSession>().last.lock() = Some(result.clone());
    let _ = app.emit("dictation-result", result);
}
#[tauri::command]
pub async fn improve_text(app: AppHandle, text: String) -> AppResult<String> {
    let _activity = crate::application::updates::activity::lease()?;
    let settings = app.state::<AppState>().settings();
    if text.trim().is_empty() || text.len() > 200_000 {
        return Err(AppError::Config(
            "Введите текст длиной до 200 000 байт".into(),
        ));
    }
    if settings.ai_mode == AiMode::Off {
        return Err(AppError::Config("Включите обработку текста".into()));
    }
    let profile = settings
        .correction_profile()
        .ok_or_else(|| AppError::Config("Выберите подключение для обработки".into()))?;
    if settings
        .text_correction_llm
        .model
        .as_ref()
        .or(profile.model.as_ref())
        .is_none()
    {
        return Err(AppError::Config("Выберите модель обработки текста".into()));
    }
    crate::llm::LlmClient::from_settings(&settings)
        .process_preset(
            &text,
            crate::application::dictation::workflow::effective_preset(&settings),
            settings.processing_target_language,
            settings.clean_prompt.as_deref(),
        )
        .await
}
#[tauri::command]
pub fn clear_speech_analytics() -> AppResult<()> {
    crate::history::apply_analytics_privacy_policy(false, 0)
}
#[tauri::command]
pub async fn recommend_speech(
    app: AppHandle,
    id: String,
) -> AppResult<crate::llm::SpeechLlmRecommendation> {
    let settings = app.state::<AppState>().settings();
    let assignment = &settings.speech_analysis_llm;
    if !settings.analytics_enabled || !settings.speech_trainer_enabled || !assignment.enabled {
        return Err(AppError::Config(
            "Включите тренера, аналитику и рекомендации ИИ".into(),
        ));
    }
    let profile = settings
        .speech_analysis_profile()
        .ok_or_else(|| AppError::Config("Выберите подключение тренера".into()))?;
    if profile.connection == LlmConnectionKind::Cloud && !assignment.cloud_consent {
        return Err(AppError::Config(
            "Подтвердите передачу данных в облако".into(),
        ));
    }
    let entry = crate::history::list(true, settings.analytics_retention_days)?
        .into_iter()
        .find(|e| e.id == id && e.analytics_included)
        .ok_or_else(|| AppError::Config("Диктовка недоступна для анализа".into()))?;
    let original = entry
        .original_text
        .ok_or_else(|| AppError::Config("Исходная расшифровка удалена".into()))?;
    let analysis = entry
        .analysis
        .unwrap_or_else(|| crate::speech_metrics::analyze_russian_speech(&original));
    let findings = match assignment.data_scope {
        SpeechLlmDataScope::MetricsOnly => None,
        _ => Some(analysis.findings.as_slice()),
    };
    let text =
        (assignment.data_scope == SpeechLlmDataScope::OriginalText).then_some(original.as_str());
    crate::llm::LlmClient::from_profile(profile, assignment.model.as_deref())
        .analyze_speech(&analysis, findings, text)
        .await
}
#[tauri::command]
pub fn get_microphone_sample(session: State<'_, DesktopSession>) -> AppResult<Vec<i16>> {
    let sample = session.microphone_sample.lock().clone();
    if sample.is_empty() {
        return Err(AppError::Audio(
            "Сначала запишите образец проверкой микрофона".into(),
        ));
    }
    Ok(sample)
}
#[tauri::command]
pub async fn test_microphone_device(
    app: AppHandle,
    device_id: Option<String>,
) -> AppResult<crate::application::diagnostics::MicTestResult> {
    crate::application::diagnostics::test_microphone_device(app, 3000, device_id).await
}
#[derive(Serialize)]
pub struct SelectedModelMetadata {
    path: String,
    filename: String,
    bytes: u64,
}
#[tauri::command]
pub fn get_selected_model_metadata(app: AppHandle) -> Option<SelectedModelMetadata> {
    let path = app.state::<AppState>().settings().whisper_model_path?;
    let file = std::path::Path::new(&path);
    let metadata = file.metadata().ok().filter(|m| m.is_file())?;
    Some(SelectedModelMetadata {
        filename: file.file_name()?.to_string_lossy().into_owned(),
        path,
        bytes: metadata.len(),
    })
}
#[tauri::command]
pub fn remove_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    let settings = app.state::<AppState>().settings();
    let model = crate::application::models::list_whisper_models()?
        .into_iter()
        .find(|m| m.size.api_identifier() == size)
        .ok_or_else(|| AppError::Config("Неизвестная модель".into()))?;
    let file = model
        .local_path
        .ok_or_else(|| AppError::Config("Модель не установлена".into()))?;
    if settings.whisper_model_path.as_deref() == Some(file.as_str()) {
        return Err(AppError::Config(
            "Сначала выберите другую модель распознавания".into(),
        ));
    }
    std::fs::remove_file(file)?;
    Ok(())
}
