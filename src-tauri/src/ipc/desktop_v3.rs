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
    pub outcome: Mutex<Option<crate::events::DictationOutcomeEventV1>>,
}

impl DesktopSession {
    /// A late result cannot replace a newer session or reverse cancellation.
    pub(crate) fn record_outcome(&self, outcome: crate::events::DictationOutcomeEventV1) -> bool {
        use crate::application::dictation::DictationOutcomeKind;
        let mut last = self.outcome.lock();
        if last.as_ref().is_some_and(|previous| {
            previous.operation_id > outcome.operation_id
                || (previous.operation_id == outcome.operation_id
                    && matches!(
                        previous.outcome,
                        DictationOutcomeKind::Cancelled | DictationOutcomeKind::Superseded
                    )
                    && previous.outcome != outcome.outcome)
        }) {
            return false;
        }
        *last = Some(outcome);
        true
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::DesktopSession;
    use crate::{
        application::dictation::DictationOutcomeKind as Kind,
        events::DictationOutcomeEventV1 as Event,
    };

    #[test]
    fn snapshot_retains_outcome_without_content_and_fences_late_results() {
        let session = DesktopSession::default();
        assert!(session.record_outcome(Event {
            operation_id: 1,
            outcome: Kind::Deferred
        }));
        assert!(session.record_outcome(Event {
            operation_id: 1,
            outcome: Kind::Completed
        }));
        assert!(session.record_outcome(Event {
            operation_id: 2,
            outcome: Kind::Cancelled
        }));
        assert!(!session.record_outcome(Event {
            operation_id: 1,
            outcome: Kind::NoSpeech
        }));
        assert!(!session.record_outcome(Event {
            operation_id: 2,
            outcome: Kind::Completed
        }));
        assert_eq!(
            *session.outcome.lock(),
            Some(Event {
                operation_id: 2,
                outcome: Kind::Cancelled
            })
        );
        assert!(session.record_outcome(Event {
            operation_id: 3,
            outcome: Kind::NoSpeech
        }));
        assert!(session.last.lock().is_none());
    }
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
    pretranscription: crate::pipeline::pretranscription::Status,
    state: PipelineState,
    level: f32,
    last: Option<LastDictation>,
    operation_id: u64,
    source: Option<fono_core::OperationSource>,
    outcome: Option<crate::events::DictationOutcomeEventV1>,
}
#[tauri::command]
pub fn get_desktop_snapshot(app: AppHandle) -> DesktopSnapshot {
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.current_operation();
    DesktopSnapshot {
        pretranscription: pipeline
            .pretranscription_status_for(operation.as_ref().map_or(0, |operation| operation.id)),
        state: Pipeline::project_state(operation.as_ref()),
        level: pipeline.current_level().clamp(0.0, 1.0),
        last: app.state::<DesktopSession>().last.lock().clone(),
        operation_id: operation.as_ref().map_or(0, |operation| operation.id),
        source: operation.map(|operation| operation.source),
        outcome: *app.state::<DesktopSession>().outcome.lock(),
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
    let preset = crate::application::dictation::workflow::effective_preset(&settings);
    let language = crate::application::dictation::workflow::effective_language(&settings);
    if preset == TextPreset::Raw && language.is_none() {
        return Ok(text);
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
    let client_settings = settings.clone();
    let client =
        super::blocking::run(move || Ok(crate::llm::LlmClient::from_settings(&client_settings)))
            .await?;
    client
        .process_preset(
            &text,
            preset,
            language,
            settings.processing_prompts.choice(preset),
        )
        .await
}
#[tauri::command]
pub async fn clear_speech_analytics(app: AppHandle) -> AppResult<()> {
    super::blocking::run(move || crate::history::for_app(&app).clear_analytics()).await
}
#[tauri::command]
pub async fn recommend_speech(
    app: AppHandle,
    id: String,
) -> AppResult<crate::llm::SpeechLlmRecommendation> {
    crate::application::trainer_ai::recommend_session(app, id).await
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
pub async fn get_selected_model_metadata(
    app: AppHandle,
) -> AppResult<Option<SelectedModelMetadata>> {
    let path = app.state::<AppState>().settings().whisper_model_path;
    super::blocking::run(move || {
        Ok(path.and_then(|path| {
            let file = std::path::Path::new(&path);
            let metadata = file.metadata().ok().filter(|m| m.is_file())?;
            Some(SelectedModelMetadata {
                filename: file.file_name()?.to_string_lossy().into_owned(),
                path,
                bytes: metadata.len(),
            })
        }))
    })
    .await
}
#[tauri::command]
pub async fn remove_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    let size =
        serde_json::from_value::<crate::types::WhisperModelSize>(serde_json::Value::String(size))
            .map_err(|_| AppError::Config("Unknown Whisper model".into()))?;
    crate::application::models::remove_stt_model(app, crate::types::ModelId::whisper(size)).await
}
