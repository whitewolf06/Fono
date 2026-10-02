//! Application use case for UI, hotkey and diagnostic dictation flows.
//!
//! The Tauri IPC modules only translate command arguments. This module owns
//! recording, operation fencing, VAD, STT, optional LLM processing, text
//! injection and history persistence for dictation.

use tauri::{AppHandle, Manager};

use crate::application::dictation_tail_diagnostics::{
    DictationStopReason, DictationTailDiagnostic,
};
use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::operation::{OperationCancellation, OperationSource, TerminalReason};
use crate::pipeline::{self, Pipeline};
use crate::state::AppState;
use crate::types::{
    AiMode, DictationAnalysisStatus, DictationHistoryEntry, DictationProcessingMetadata,
    PipelineState, Settings, Transcript,
};

fn set_pipeline_idle(app: &AppHandle, state: &AppState) {
    pipeline::set_state(app, state, PipelineState::Idle);
}

fn set_pipeline_state_for_operation(
    app: &AppHandle,
    state: &AppState,
    pipeline: &Pipeline,
    operation: u64,
    new: PipelineState,
    terminal_reason: TerminalReason,
) -> bool {
    pipeline::set_state_for_operation(app, state, pipeline, operation, new, terminal_reason)
}

fn empty_transcript() -> Transcript {
    Transcript {
        text: String::new(),
        detected_language: None,
        transcribe_secs: None,
        audio_secs: None,
        device: None,
    }
}

fn speech_trainer_collection_enabled(settings: &Settings) -> bool {
    settings.analytics_enabled && settings.speech_trainer_enabled
}

/// A cancel cannot synchronously stop CPU Whisper or an HTTP request.  Instead,
/// every result is fenced by the operation id that created it.
fn operation_still_active(pipeline: &Pipeline, operation: u64, context: &str) -> bool {
    let active = pipeline.is_operation_active(operation);
    if !active {
        tracing::info!("{context}: result discarded because dictation was cancelled or replaced");
    }
    active
}

pub(crate) async fn wait_for_cancellation(cancellation: OperationCancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn emit_pipeline_error(app: &AppHandle, message: &str) {
    crate::events::emit_error(
        app,
        crate::events::ErrorCodeV1::Internal,
        message,
        app.state::<Pipeline>()
            .current_operation()
            .map(|item| item.id),
    );
}

// ====== Состояние конвейера ======

pub(crate) fn start(app: AppHandle, source: OperationSource) -> AppResult<()> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();

    if state.is_dictation_paused() {
        let msg = "Запись приостановлена (через меню/траи).";
        emit_pipeline_error(&app, msg);
        return Err(AppError::Config(msg.to_string()));
    }

    if let Err(e) = pipeline.start_recording_from(settings.audio_device_id.as_deref(), source) {
        emit_pipeline_error(&app, &e.to_string());
        tracing::error!("start_dictation: start_recording FAILED: {e}");
        return Err(e);
    }
    pipeline::set_state(&app, state.inner(), PipelineState::Listening);
    arm_recording_safety_timeout(app.clone(), pipeline.operation_id());
    Ok(())
}

fn arm_recording_safety_timeout(app: AppHandle, operation: u64) {
    tauri::async_runtime::spawn(async move {
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(pipeline::MAX_RECORDING_SECONDS as u64);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            tokio::time::sleep(remaining.min(std::time::Duration::from_secs(1))).await;

            let pipeline = app.state::<Pipeline>();
            if !pipeline.is_operation_active(operation) || !pipeline.is_recording() {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }

        let pipeline = app.state::<Pipeline>();
        tracing::warn!(
            operation,
            max_seconds = pipeline::MAX_RECORDING_SECONDS,
            "recording safety timeout reached; cancelling the operation"
        );
        if let Some(event) = pipeline.cancel() {
            crate::events::emit_operation(&app, event);
        }
        let _ = pipeline.stop_recording();
        set_pipeline_idle(&app, app.state::<AppState>().inner());
        app.state::<fono_wake::WakeWordHandle>().resume();
        emit_pipeline_error(
            &app,
            &format!(
                "Recording was cancelled after the {} second safety limit.",
                pipeline::MAX_RECORDING_SECONDS
            ),
        );
    });
}

pub(crate) async fn stop(app: AppHandle) -> AppResult<Transcript> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();
    let operation = pipeline.operation_id();
    let source = pipeline
        .current_operation()
        .map(|snapshot| snapshot.source)
        .unwrap_or(OperationSource::Ui);
    let mut tail_diagnostic =
        DictationTailDiagnostic::new(operation, source, DictationStopReason::Manual);
    let cancellation = pipeline
        .cancellation(operation)
        .ok_or_else(|| AppError::Internal("active dictation has no cancellation signal".into()))?;

    // Always release the microphone before model loading or transcription.
    let samples = match pipeline.stop_recording() {
        Ok(s) => s,
        Err(e) => {
            tail_diagnostic.emit();
            emit_pipeline_error(&app, &e.to_string());
            tracing::error!("stop_dictation: stop_recording FAILED: {e}");
            let _ = set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(e);
        }
    };
    tail_diagnostic.record_capture(samples.len());
    if pipeline.recording_limit_reached() {
        emit_pipeline_error(
            &app,
            &format!(
                "Recording reached the {} second safety limit; the captured part will be transcribed.",
                pipeline::MAX_RECORDING_SECONDS
            ),
        );
    }
    if !operation_still_active(&pipeline, operation, "stop_dictation after recording") {
        tail_diagnostic.stt_cancelled();
        tail_diagnostic.emit();
        return Ok(empty_transcript());
    }
    if samples.is_empty() {
        tail_diagnostic.emit();
        let _ = set_pipeline_state_for_operation(
            &app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(empty_transcript());
    }

    // Offline VAD removes only leading silence here. Unlike a full trim, it
    // preserves every captured sample after speech starts, so a quiet final
    // word cannot be cut when the user releases the global hotkey.
    let captured_samples = samples.len();
    let (samples, vad) = crate::vad::trim_leading_silence_with_result(&samples);
    let retained_trailing_samples = captured_samples.saturating_sub(vad.speech_end_sample);
    tail_diagnostic.record_vad(
        captured_samples,
        samples.len(),
        &vad,
        retained_trailing_samples,
    );
    if samples.is_empty() {
        tail_diagnostic.emit();
        tracing::info!("VAD: речь не обнаружена вообще — пропускаем транскрипцию");
        let _ = set_pipeline_state_for_operation(
            &app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(empty_transcript());
    }

    if !set_pipeline_state_for_operation(
        &app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Transcribing,
        TerminalReason::Completed,
    ) {
        tail_diagnostic.stt_cancelled();
        return Ok(empty_transcript());
    }
    if let Some(path) = settings.whisper_model_path.as_deref() {
        let stt = pipeline.stt().clone();
        let path = std::path::PathBuf::from(path);
        let acceleration = settings.acceleration;
        let worker_paths = crate::stt::worker_paths_for_app(&app);
        let load = tauri::async_runtime::spawn_blocking(move || {
            stt.ensure_loaded(&path, acceleration, &worker_paths)
        });
        let load_result = tokio::select! {
            result = load => Some(result),
            _ = wait_for_cancellation(cancellation.clone()) => None,
        };
        let Some(load_result) = load_result else {
            tail_diagnostic.stt_cancelled();
            tail_diagnostic.emit();
            tracing::info!(
                operation,
                "model load detached after dictation cancellation"
            );
            return Ok(empty_transcript());
        };
        let load_result = match load_result {
            Ok(result) => result,
            Err(error) => {
                tail_diagnostic.stt_failed();
                return Err(AppError::Internal(format!("model load join: {error}")));
            }
        };
        if let Err(error) = load_result {
            tail_diagnostic.stt_failed();
            tail_diagnostic.emit();
            emit_pipeline_error(&app, &error.to_string());
            let _ = set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(error);
        }
    } else {
        tail_diagnostic.stt_failed();
        tail_diagnostic.emit();
        let error_msg =
            "Whisper model is not selected. Download and choose a model in settings.".to_string();
        emit_pipeline_error(&app, &error_msg);
        let _ = set_pipeline_state_for_operation(
            &app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Failed,
        );
        return Err(AppError::Stt(error_msg));
    }
    if !operation_still_active(&pipeline, operation, "stop_dictation after model load") {
        tail_diagnostic.stt_cancelled();
        tail_diagnostic.emit();
        return Ok(empty_transcript());
    }

    // Транскрибируем (CPU-bound — запускаем в spawn_blocking).
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let app_for_err = app.clone();
    let stt_cancellation = cancellation.clone();
    let transcribe = tauri::async_runtime::spawn_blocking(move || {
        stt.transcribe_cancellable(&samples, &language, stt_cancellation)
    });
    let transcript_result = tokio::select! {
        result = transcribe => Some(result),
        _ = wait_for_cancellation(cancellation.clone()) => None,
    };
    let Some(transcript_result) = transcript_result else {
        tail_diagnostic.stt_cancelled();
        tail_diagnostic.emit();
        tracing::info!(operation, "STT wait interrupted by dictation cancellation");
        return Ok(empty_transcript());
    };
    let transcript = transcript_result
        .map_err(|e| {
            tail_diagnostic.stt_failed();
            tail_diagnostic.emit();
            crate::events::emit_error(
                &app_for_err,
                crate::events::ErrorCodeV1::Stt,
                e.to_string(),
                Some(operation),
            );
            let _ = set_pipeline_state_for_operation(
                &app_for_err,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            AppError::Internal(format!("transcribe join: {e}"))
        })?
        .map_err(|e| {
            tail_diagnostic.stt_failed();
            tail_diagnostic.emit();
            crate::events::emit_error(
                &app,
                crate::events::ErrorCodeV1::Stt,
                e.to_string(),
                Some(operation),
            );
            let _ = set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            e
        })?;
    tail_diagnostic.stt_succeeded(!transcript.text.trim().is_empty());

    if !operation_still_active(&pipeline, operation, "stop_dictation after transcription") {
        tail_diagnostic.emit();
        return Ok(empty_transcript());
    }

    tracing::info!(
        "transcript ready ({} chars)",
        transcript.text.chars().count()
    );

    // Опциональная AI-обработка.
    let final_text = match settings.ai_mode {
        AiMode::Off => {
            tail_diagnostic.postprocessor_skipped();
            transcript.text.clone()
        }
        mode => {
            if !set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Processing,
                TerminalReason::Completed,
            ) {
                tail_diagnostic.postprocessor_cancelled();
                tail_diagnostic.emit();
                return Ok(empty_transcript());
            }
            let client = LlmClient::from_settings(&settings);
            let process = client.process(&transcript.text, mode, settings.clean_prompt.as_deref());
            let result = tokio::select! {
                result = process => Some(result),
                _ = wait_for_cancellation(cancellation.clone()) => None,
            };
            let Some(result) = result else {
                tail_diagnostic.postprocessor_cancelled();
                tail_diagnostic.emit();
                tracing::info!(
                    operation,
                    "LLM request interrupted by dictation cancellation"
                );
                return Ok(empty_transcript());
            };
            match result {
                Ok(t) => {
                    tail_diagnostic.postprocessor_succeeded(t != transcript.text);
                    t
                }
                Err(e) => {
                    tail_diagnostic.postprocessor_fallback();
                    tracing::warn!("LLM failed ({e}) — returning raw transcript");
                    crate::events::emit_error(
                        &app,
                        crate::events::ErrorCodeV1::Llm,
                        format!("LLM: {e}"),
                        Some(operation),
                    );
                    transcript.text.clone()
                }
            }
        }
    };

    if !operation_still_active(&pipeline, operation, "stop_dictation before injection") {
        tail_diagnostic.emit();
        return Ok(empty_transcript());
    }

    crate::vlog!(
        "dictation final text ready ({} chars)",
        final_text.chars().count()
    );

    let history_id = crate::history::next_id();
    // Publish the current session independently from archive persistence and insertion.
    crate::ipc::desktop_v3::publish_result(
        &app,
        crate::ipc::desktop_v3::LastDictation {
            id: history_id.clone(),
            text: final_text.clone(),
            original_text: transcript.text.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            audio_secs: f64::from(transcript.audio_secs.unwrap_or(0.0)),
        },
    );

    // A recording started in the main window returns text to its editor.
    // Hotkey and wake dictation retain insertion into the foreground application.
    if source != OperationSource::Ui {
        if !set_pipeline_state_for_operation(
            &app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Injecting,
            TerminalReason::Completed,
        ) {
            tail_diagnostic.emit();
            return Ok(empty_transcript());
        }
        if let Err(e) = crate::injection::inject_text(&final_text, settings.injection_mode) {
            emit_pipeline_error(&app, &e.to_string());
            let _ = set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(e);
        }
    }
    let _ = set_pipeline_state_for_operation(
        &app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );

    if !final_text.trim().is_empty() && settings.history_enabled {
        let trainer_collection_enabled = speech_trainer_collection_enabled(&settings);
        let analytics_payload = trainer_collection_enabled.then(|| {
            (
                transcript.text.clone(),
                DictationProcessingMetadata {
                    ai_mode: settings.ai_mode,
                    detected_language: transcript.detected_language.clone(),
                    transcribe_secs: transcript.transcribe_secs,
                    audio_secs: transcript.audio_secs,
                },
            )
        });
        if let Err(error) = crate::history::append(
            DictationHistoryEntry {
                id: history_id.clone(),
                text: final_text.clone(),
                created_at: chrono::Utc::now(),
                device: transcript.device.clone(),
                analytics_included: trainer_collection_enabled,
                original_text: analytics_payload.as_ref().map(|(text, _)| text.clone()),
                processing: analytics_payload.map(|(_, processing)| processing),
                analysis_status: if trainer_collection_enabled {
                    DictationAnalysisStatus::Pending
                } else {
                    DictationAnalysisStatus::Disabled
                },
                analysis: None,
                analysis_error: None,
                recommendation_status: if trainer_collection_enabled
                    && settings.speech_analysis_llm.enabled
                {
                    DictationAnalysisStatus::Pending
                } else {
                    DictationAnalysisStatus::Disabled
                },
                recommendation: None,
                recommendation_error: None,
            },
            settings.analytics_enabled,
            settings.analytics_retention_days,
        ) {
            tracing::warn!(operation, "dictation history persistence failed: {error}");
            crate::events::emit_error(
                &app,
                crate::events::ErrorCodeV1::Internal,
                format!("Не удалось сохранить историю диктовки: {error}"),
                Some(operation),
            );
        } else if trainer_collection_enabled {
            app.state::<crate::application::speech_analysis_queue::SpeechAnalysisQueue>()
                .enqueue(history_id);
        }
    }

    tail_diagnostic.emit();

    Ok(Transcript {
        text: final_text,
        detected_language: transcript.detected_language,
        transcribe_secs: transcript.transcribe_secs,
        audio_secs: transcript.audio_secs,
        device: transcript.device,
    })
}

#[cfg(test)]
mod tests {
    use super::speech_trainer_collection_enabled;
    use crate::types::Settings;

    #[test]
    fn speech_trainer_collection_requires_consent_and_the_trainer_switch() {
        let mut settings = Settings::default();
        assert!(!speech_trainer_collection_enabled(&settings));

        settings.analytics_enabled = true;
        assert!(speech_trainer_collection_enabled(&settings));

        settings.speech_trainer_enabled = false;
        assert!(!speech_trainer_collection_enabled(&settings));
    }
}

/// Тестовая команда: записать `duration_ms` миллисекунд и распознать.
///
/// Не делает injection в окно — возвращает транскрипт вызывающему (UI).
/// Используется кнопкой «🧪 Записать и распознать» в настройках для проверки
/// аудио-конвейера на машине пользователя.
///
/// `inject=true` — после распознавания текст вставляется в активное окно через
/// SendInput (Этап 2: текст-инъекция). `inject=false` — только возвращает
/// транскрипт для отображения в UI (безопасно для теста).
pub(crate) async fn transcribe_test(
    app: AppHandle,
    duration_ms: u64,
    inject: Option<bool>,
) -> AppResult<Transcript> {
    let inject = inject.unwrap_or(false);
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();

    let model_path = if let Some(path) = settings.whisper_model_path.as_deref() {
        std::path::PathBuf::from(path)
    } else {
        let error_msg =
            "Whisper model is not selected. Download and choose a model in settings.".to_string();
        emit_pipeline_error(&app, &error_msg);
        return Err(AppError::Stt(error_msg));
    };

    // Старт записи.
    tracing::info!(
        "transcribe_test: starting recording (device_id={:?})",
        settings.audio_device_id
    );
    if let Err(e) = pipeline.start_recording_from(
        settings.audio_device_id.as_deref(),
        OperationSource::Diagnostics,
    ) {
        emit_pipeline_error(&app, &e.to_string());
        tracing::error!("transcribe_test: start_recording FAILED: {e}");
        return Err(e);
    }
    pipeline::set_state(&app, state.inner(), PipelineState::Listening);
    let operation = pipeline.operation_id();
    let cancellation = pipeline.cancellation(operation).ok_or_else(|| {
        AppError::Internal("active diagnostic dictation has no cancellation signal".into())
    })?;
    tracing::info!(
        "transcribe_test: recording started, sleeping {} ms",
        duration_ms
    );

    // Ждём указанную длительность.
    let dur = std::time::Duration::from_millis(duration_ms.clamp(500, 30_000));
    tokio::select! {
        _ = tokio::time::sleep(dur) => {}
        _ = wait_for_cancellation(cancellation.clone()) => {
            tracing::info!(operation, "diagnostic recording wait cancelled");
            return Ok(empty_transcript());
        }
    }

    if !operation_still_active(&pipeline, operation, "transcribe_test after recording") {
        return Ok(empty_transcript());
    }

    tracing::info!("transcribe_test: sleep done, stopping recording");
    // Стоп и забираем сэмплы.
    let samples = match pipeline.stop_recording() {
        Ok(s) => s,
        Err(e) => {
            emit_pipeline_error(&app, &e.to_string());
            tracing::error!("transcribe_test: stop_recording FAILED: {e}");
            let _ = set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(e);
        }
    };
    let sample_count = samples.len();
    tracing::info!(
        "captured {} samples (~{:.1}s @ 16kHz)",
        sample_count,
        sample_count as f32 / 16_000.0
    );

    if sample_count < 1600 {
        emit_pipeline_error(
            &app,
            "Test recording is too short or too quiet. Please speak closer and longer.",
        );
        // < 0.1 сек — что-то не так с микрофоном
        let _ = set_pipeline_state_for_operation(
            &app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Failed,
        );
        return Err(AppError::Audio(
            "записано слишком мало аудио — проверьте, что микрофон работает и не занят другим приложением".into(),
        ));
    }

    if !set_pipeline_state_for_operation(
        &app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Transcribing,
        TerminalReason::Completed,
    ) {
        return Ok(empty_transcript());
    }
    let stt = pipeline.stt().clone();
    let acceleration = settings.acceleration;
    let worker_paths = crate::stt::worker_paths_for_app(&app);
    let load = tauri::async_runtime::spawn_blocking(move || {
        stt.ensure_loaded(&model_path, acceleration, &worker_paths)
    });
    let load_result = tokio::select! {
        result = load => Some(result),
        _ = wait_for_cancellation(cancellation.clone()) => None,
    };
    let Some(load_result) = load_result else {
        tracing::info!(operation, "diagnostic model load wait cancelled");
        return Ok(empty_transcript());
    };
    load_result.map_err(|error| AppError::Internal(format!("model load join: {error}")))??;

    // Транскрипция (CPU-bound).
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let app_for_err = app.clone();
    let stt_cancellation = cancellation.clone();
    let transcribe = tauri::async_runtime::spawn_blocking(move || {
        stt.transcribe_cancellable(&samples, &language, stt_cancellation)
    });
    let transcript_result = tokio::select! {
        result = transcribe => Some(result),
        _ = wait_for_cancellation(cancellation.clone()) => None,
    };
    let Some(transcript_result) = transcript_result else {
        tracing::info!(operation, "diagnostic STT wait cancelled");
        return Ok(empty_transcript());
    };
    let transcript = transcript_result
        .map_err(|e| {
            crate::events::emit_error(
                &app_for_err,
                crate::events::ErrorCodeV1::Stt,
                e.to_string(),
                Some(operation),
            );
            let _ = set_pipeline_state_for_operation(
                &app_for_err,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            AppError::Internal(format!("transcribe join: {e}"))
        })?
        .map_err(|e| {
            crate::events::emit_error(
                &app,
                crate::events::ErrorCodeV1::Stt,
                e.to_string(),
                Some(operation),
            );
            let _ = set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            e
        })?;

    if !operation_still_active(&pipeline, operation, "transcribe_test after transcription") {
        return Ok(empty_transcript());
    }

    tracing::info!(
        "test transcript ready ({} chars)",
        transcript.text.chars().count()
    );

    // Опциональная AI-обработка — но на ошибке не падаем.
    let final_text = match settings.ai_mode {
        AiMode::Off => transcript.text.clone(),
        mode => {
            if !set_pipeline_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Processing,
                TerminalReason::Completed,
            ) {
                return Ok(empty_transcript());
            }
            let client = LlmClient::from_settings(&settings);
            let process = client.process(&transcript.text, mode, settings.clean_prompt.as_deref());
            let result = tokio::select! {
                result = process => Some(result),
                _ = wait_for_cancellation(cancellation.clone()) => None,
            };
            let Some(result) = result else {
                tracing::info!(operation, "diagnostic LLM request cancelled");
                return Ok(empty_transcript());
            };
            match result {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("LLM failed ({e}) — returning raw transcript");
                    transcript.text.clone()
                }
            }
        }
    };

    if !operation_still_active(
        &pipeline,
        operation,
        "transcribe_test before returning result",
    ) {
        return Ok(empty_transcript());
    }

    // Этап 2: текст-инъекция в активное окно через SendInput.
    if inject && !final_text.is_empty() {
        if !set_pipeline_state_for_operation(
            &app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Injecting,
            TerminalReason::Completed,
        ) {
            return Ok(empty_transcript());
        }
        match crate::injection::inject_text(&final_text, settings.injection_mode) {
            Ok(()) => tracing::info!(
                "injected {} chars into active window",
                final_text.chars().count()
            ),
            Err(e) => {
                tracing::warn!("injection failed ({e}) — returning transcript anyway");
                crate::events::emit_error(
                    &app,
                    crate::events::ErrorCodeV1::Injection,
                    format!("Вставка текста: {e}"),
                    Some(operation),
                );
            }
        }
    }

    let _ = set_pipeline_state_for_operation(
        &app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );

    Ok(Transcript {
        text: final_text,
        detected_language: transcript.detected_language,
        transcribe_secs: transcript.transcribe_secs,
        audio_secs: transcript.audio_secs,
        device: transcript.device,
    })
}
