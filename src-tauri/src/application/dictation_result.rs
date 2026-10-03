//! Final results are independent of the archive and shared by every source.
use crate::types::{
    DictationAnalysisStatus, DictationHistoryEntry, DictationProcessingMetadata, Settings,
    Transcript,
};
use tauri::{AppHandle, Manager};

pub fn publish(app: &AppHandle, id: &str, transcript: &Transcript, text: &str) {
    crate::ipc::desktop_v3::publish_result(
        app,
        crate::ipc::desktop_v3::LastDictation {
            id: id.into(),
            text: text.into(),
            original_text: transcript.text.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            audio_secs: f64::from(transcript.audio_secs.unwrap_or(0.0)),
        },
    );
}

pub fn archive(
    app: &AppHandle,
    settings: &Settings,
    id: String,
    transcript: &Transcript,
    final_text: &str,
    operation: u64,
) {
    if final_text.trim().is_empty() || !settings.history_enabled {
        return;
    }
    let analytics = settings.analytics_enabled && settings.speech_trainer_enabled;
    let status = if analytics {
        DictationAnalysisStatus::Pending
    } else {
        DictationAnalysisStatus::Disabled
    };
    let entry = DictationHistoryEntry {
        id: id.clone(),
        text: final_text.into(),
        created_at: chrono::Utc::now(),
        device: transcript.device.clone(),
        analytics_included: analytics,
        original_text: analytics.then(|| transcript.text.clone()),
        processing: analytics.then(|| DictationProcessingMetadata {
            ai_mode: settings.ai_mode,
            detected_language: transcript.detected_language.clone(),
            transcribe_secs: transcript.transcribe_secs,
            audio_secs: transcript.audio_secs,
        }),
        analysis_status: status,
        analysis: None,
        analysis_error: None,
        recommendation_status: if analytics && settings.speech_analysis_llm.enabled {
            DictationAnalysisStatus::Pending
        } else {
            DictationAnalysisStatus::Disabled
        },
        recommendation: None,
        recommendation_error: None,
    };
    match crate::history::append(
        entry,
        settings.analytics_enabled,
        settings.analytics_retention_days,
    ) {
        Ok(()) if analytics => app
            .state::<crate::application::speech_analysis_queue::SpeechAnalysisQueue>()
            .enqueue(id),
        Ok(()) => (),
        Err(error) => crate::events::emit_error(
            app,
            crate::events::ErrorCodeV1::Internal,
            format!("Не удалось сохранить историю диктовки: {error}"),
            Some(operation),
        ),
    }
}
