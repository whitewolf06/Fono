//! Tauri IPC команды — мост между фронтендом и Rust-ядром.
//!
//! Каждая команда доступна из JS через `invoke('<name>', { args })`.
//! Список команд см. в `docs/architecture.md` → `commands.rs`.
//!
//! В async-командах мы используем `AppHandle::state::<T>()` вместо
//! `State<'_, T>`, чтобы не удерживать borrow через `.await`.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::operation::{OperationSource, TerminalReason};
use crate::pipeline::{self, Pipeline};
use crate::state::{self, AppState};
use crate::types::{AiMode, DictationHistoryEntry, PipelineState, Transcript};

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

/// A cancel cannot synchronously stop CPU Whisper or an HTTP request.  Instead,
/// every result is fenced by the operation id that created it.
fn operation_still_active(pipeline: &Pipeline, operation: u64, context: &str) -> bool {
    let active = pipeline.is_operation_active(operation);
    if !active {
        tracing::info!("{context}: result discarded because dictation was cancelled or replaced");
    }
    active
}

fn emit_pipeline_error(app: &AppHandle, message: &str) {
    let _ = app.emit("error", message);
}

static WAKE_WORD_TEST_AUDIO: Lazy<Mutex<Vec<i16>>> = Lazy::new(|| Mutex::new(Vec::new()));

// ====== Состояние конвейера ======

pub(crate) fn start_dictation_from(app: AppHandle, source: OperationSource) -> AppResult<()> {
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
            let _ = app.emit("operation-state", event);
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

pub(crate) async fn stop_dictation(app: AppHandle) -> AppResult<Transcript> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();
    let operation = pipeline.operation_id();

    // Always release the microphone before model loading or transcription.
    let samples = match pipeline.stop_recording() {
        Ok(s) => s,
        Err(e) => {
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
        return Ok(empty_transcript());
    }
    if samples.is_empty() {
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

    // VAD: обрезаем тишину в начале/конце — whisper получит меньше аудио,
    // значит отработает быстрее (ускорение 1.5-2x на типичной записи).
    let samples = crate::vad::trim_silence(&samples);
    if samples.is_empty() {
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
        return Ok(empty_transcript());
    }
    if let Some(path) = settings.whisper_model_path.as_deref() {
        if let Err(error) = pipeline.stt().ensure_loaded(
            std::path::Path::new(path),
            settings.acceleration,
            &crate::stt::worker_paths_for_app(&app),
        ) {
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
        return Ok(empty_transcript());
    }

    // Транскрибируем (CPU-bound — запускаем в spawn_blocking).
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let app_for_err = app.clone();
    let transcript =
        tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
            .await
            .map_err(|e| {
                let _ = app_for_err.emit("error", e.to_string());
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
                let _ = app.emit("error", e.to_string());
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

    if !operation_still_active(&pipeline, operation, "stop_dictation after transcription") {
        return Ok(empty_transcript());
    }

    tracing::info!(
        "transcript ready ({} chars)",
        transcript.text.chars().count()
    );

    // Опциональная AI-обработка.
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
            match client
                .process(&transcript.text, mode, settings.clean_prompt.as_deref())
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("LLM failed ({e}) — returning raw transcript");
                    let _ = app.emit("error", format!("LLM: {e}"));
                    transcript.text.clone()
                }
            }
        }
    };

    if !operation_still_active(&pipeline, operation, "stop_dictation before injection") {
        return Ok(empty_transcript());
    }

    crate::vlog!(
        "dictation final text ready ({} chars)",
        final_text.chars().count()
    );

    // Вставка текста.
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
    let _ = set_pipeline_state_for_operation(
        &app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );

    if !final_text.trim().is_empty() {
        let _ = state::append_dictation_history(DictationHistoryEntry {
            id: format!("{}", chrono::Utc::now().timestamp_millis()),
            text: final_text.clone(),
            created_at: chrono::Utc::now(),
            device: transcript.device.clone(),
        });
    }

    Ok(Transcript {
        text: final_text,
        detected_language: transcript.detected_language,
        transcribe_secs: transcript.transcribe_secs,
        audio_secs: transcript.audio_secs,
        device: transcript.device,
    })
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

    // Проверка модели.
    if let Some(path) = settings.whisper_model_path.as_deref() {
        pipeline.stt().ensure_loaded(
            std::path::Path::new(path),
            settings.acceleration,
            &crate::stt::worker_paths_for_app(&app),
        )?;
    } else {
        let error_msg =
            "Whisper model is not selected. Download and choose a model in settings.".to_string();
        emit_pipeline_error(&app, &error_msg);
        set_pipeline_idle(&app, state.inner());
        return Err(AppError::Stt(error_msg));
    }

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
    tracing::info!(
        "transcribe_test: recording started, sleeping {} ms",
        duration_ms
    );

    // Ждём указанную длительность.
    let dur = std::time::Duration::from_millis(duration_ms.clamp(500, 30_000));
    tokio::time::sleep(dur).await;

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
            set_pipeline_idle(&app, state.inner());
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
        set_pipeline_idle(&app, state.inner());
        return Err(AppError::Audio(
            "записано слишком мало аудио — проверьте, что микрофон работает и не занят другим приложением".into(),
        ));
    }

    // Транскрипция (CPU-bound).
    pipeline::set_state(&app, state.inner(), PipelineState::Transcribing);
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let app_for_err = app.clone();
    let transcript =
        tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
            .await
            .map_err(|e| {
                let _ = app_for_err.emit("error", e.to_string());
                set_pipeline_idle(&app_for_err, state.inner());
                AppError::Internal(format!("transcribe join: {e}"))
            })?
            .map_err(|e| {
                let _ = app.emit("error", e.to_string());
                set_pipeline_idle(&app, state.inner());
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
            pipeline::set_state(&app, state.inner(), PipelineState::Processing);
            let client = LlmClient::from_settings(&settings);
            match client
                .process(&transcript.text, mode, settings.clean_prompt.as_deref())
                .await
            {
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
        pipeline::set_state(&app, state.inner(), PipelineState::Injecting);
        match crate::injection::inject_text(&final_text, settings.injection_mode) {
            Ok(()) => tracing::info!(
                "injected {} chars into active window",
                final_text.chars().count()
            ),
            Err(e) => {
                tracing::warn!("injection failed ({e}) — returning transcript anyway");
                let _ = app.emit("error", format!("Вставка текста: {e}"));
            }
        }
    }

    set_pipeline_idle(&app, state.inner());

    Ok(Transcript {
        text: final_text,
        detected_language: transcript.detected_language,
        transcribe_secs: transcript.transcribe_secs,
        audio_secs: transcript.audio_secs,
        device: transcript.device,
    })
}

// ====== Аудио ======

// ====== Wake word ======

#[derive(Debug, serde::Serialize)]
pub struct WakeWordSampleReport {
    pub samples: usize,
    pub duration_ms: u64,
    pub rms: f32,
    pub peak: f32,
}

#[derive(Debug, serde::Serialize)]
pub struct WakeWordRecognitionReport {
    pub backend: String,
    pub detected: bool,
    pub recognized: String,
    pub json: String,
    pub audio_duration_ms: u64,
    pub processing_ms: u64,
}

/// Records a user-controlled wake-word sample through the same shared audio
/// path as dictation and live wake word. The live detector is paused so it
/// cannot consume the test phrase as a real command.
pub(crate) async fn record_wake_word_sample(
    app: AppHandle,
    duration_ms: u64,
) -> AppResult<WakeWordSampleReport> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();
    if pipeline.is_recording() {
        return Err(AppError::Audio("уже идёт другая запись".into()));
    }

    let settings = state.settings();
    wake_handle.pause();
    if let Err(error) = pipeline.start_recording_from(
        settings.audio_device_id.as_deref(),
        OperationSource::Diagnostics,
    ) {
        wake_handle.resume();
        return Err(error);
    }
    pipeline::set_state(&app, state.inner(), PipelineState::Listening);

    let duration = std::time::Duration::from_millis(duration_ms.clamp(1_000, 10_000));
    tokio::time::sleep(duration).await;
    let samples = pipeline.stop_recording();
    set_pipeline_idle(&app, state.inner());
    wake_handle.resume();
    let samples = samples?;
    if samples.is_empty() {
        return Err(AppError::Audio("тестовая запись пуста".into()));
    }

    let (rms, peak) = normalized_levels(&samples);
    *WAKE_WORD_TEST_AUDIO.lock() = samples.clone();
    Ok(WakeWordSampleReport {
        samples: samples.len(),
        duration_ms: samples.len() as u64 * 1_000 / 16_000,
        rms,
        peak,
    })
}

/// Runs the saved microphone sample through the currently selected wake-word
/// backend without listening continuously.
pub(crate) async fn recognize_wake_word_sample(
    app: AppHandle,
) -> AppResult<WakeWordRecognitionReport> {
    let samples = WAKE_WORD_TEST_AUDIO.lock().clone();
    if samples.is_empty() {
        return Err(AppError::Audio("сначала запишите тестовую фразу".into()));
    }
    let settings = app.state::<AppState>().settings();
    let config = crate::settings_to_wake_config(&settings)?;
    let backend = match settings.wake_backend {
        fono_wake::WakeWordBackend::WhisperExperimental => "Whisper Small",
        fono_wake::WakeWordBackend::SherpaOnnx => "Sherpa-ONNX",
        fono_wake::WakeWordBackend::Mock => "Mock",
        fono_wake::WakeWordBackend::Disabled => "Disabled",
    }
    .to_string();
    let audio_duration_ms = samples.len() as u64 * 1_000 / 16_000;
    let started = std::time::Instant::now();
    tracing::info!(
        backend = %backend,
        phrase = %settings.wake_word,
        samples = samples.len(),
        audio_duration_ms,
        "wake word recorded-sample recognition started"
    );

    let result = match settings.wake_backend {
        fono_wake::WakeWordBackend::WhisperExperimental => {
            recognize_whisper_sample(config, samples).await?
        }
        fono_wake::WakeWordBackend::SherpaOnnx => recognize_sherpa_sample(config, samples).await?,
        _ => {
            return Err(AppError::Internal(
                "тест записи поддерживается для Whisper и Sherpa-ONNX".into(),
            ));
        }
    };

    tracing::info!(
        backend = %backend,
        detected = result.detected,
        keyword = %result.keyword,
        processing_ms = started.elapsed().as_millis(),
        "wake word recorded-sample recognition finished"
    );
    Ok(WakeWordRecognitionReport {
        backend,
        detected: result.detected,
        recognized: result.keyword,
        json: result.json,
        audio_duration_ms,
        processing_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(feature = "whisper-wake")]
async fn recognize_whisper_sample(
    config: fono_wake::WakeWordConfig,
    samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    Ok(tauri::async_runtime::spawn_blocking(move || {
        fono_wake::test_whisper_with_samples(&config, &samples)
    })
    .await
    .map_err(|error| AppError::Internal(format!("wake test join: {error}")))??)
}

#[cfg(not(feature = "whisper-wake"))]
async fn recognize_whisper_sample(
    _config: fono_wake::WakeWordConfig,
    _samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    Err(AppError::Internal(
        "whisper-wake backend не собран в эту сборку".into(),
    ))
}

#[cfg(feature = "sherpa-wake")]
async fn recognize_sherpa_sample(
    config: fono_wake::WakeWordConfig,
    samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    let wav_path = state::app_data_dir()?.join("wake-word-test.wav");
    write_pcm16_wav(&wav_path, &samples, 16_000)?;
    Ok(tauri::async_runtime::spawn_blocking(move || {
        fono_wake::test_with_wav(&config, &wav_path, false)
    })
    .await
    .map_err(|error| AppError::Internal(format!("wake test join: {error}")))??)
}

#[cfg(not(feature = "sherpa-wake"))]
async fn recognize_sherpa_sample(
    _config: fono_wake::WakeWordConfig,
    _samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    Err(AppError::Internal(
        "sherpa-wake backend не собран в эту сборку".into(),
    ))
}

fn normalized_levels(samples: &[i16]) -> (f32, f32) {
    let mut sum = 0.0_f64;
    let mut peak = 0.0_f32;
    for &sample in samples {
        let normalized = sample as f32 / i16::MAX as f32;
        sum += (normalized as f64) * (normalized as f64);
        peak = peak.max(normalized.abs());
    }
    (((sum / samples.len().max(1) as f64) as f32).sqrt(), peak)
}

#[cfg(feature = "sherpa-wake")]
fn write_pcm16_wav(path: &std::path::Path, samples: &[i16], sample_rate: u32) -> AppResult<()> {
    let data_len = std::mem::size_of_val(samples) as u32;
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&(sample_rate * 2).to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}
