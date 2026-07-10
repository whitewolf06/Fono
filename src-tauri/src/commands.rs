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
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio::AudioCapture;
use crate::error::{AppError, AppResult};
use crate::llm::LlmClient;
use crate::pipeline::{self, Pipeline};
use crate::state::{self, AppState};
use crate::types::{
    AiMode, DeviceInfo, PipelineState, Settings, Transcript, WhisperModelInfo, WhisperModelSize,
};

fn set_pipeline_idle(app: &AppHandle, state: &AppState) {
    pipeline::set_state(app, state, PipelineState::Idle);
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

#[tauri::command]
pub fn get_pipeline_state(state: State<'_, AppState>) -> PipelineState {
    state.pipeline_state()
}

#[tauri::command]
pub fn start_dictation(app: AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();

    if state.is_dictation_paused() {
        let msg = "Запись приостановлена (через меню/траи).";
        emit_pipeline_error(&app, msg);
        return Err(AppError::Config(msg.to_string()));
    }

    pipeline::set_state(&app, &state.inner(), PipelineState::Listening);
    if let Err(e) = pipeline.start_recording(settings.audio_device_id.as_deref()) {
        emit_pipeline_error(&app, &e.to_string());
        tracing::error!("start_dictation: start_recording FAILED: {e}");
        set_pipeline_idle(&app, &state.inner());
        return Err(e);
    }
    Ok(())
}

#[tauri::command]
pub fn confirm_dictation(app: AppHandle) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    pipeline.confirm();
    tracing::info!("dictation confirmed by overlay");
    Ok(())
}

#[tauri::command]
pub fn cancel_dictation(app: AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();

    pipeline.cancel();
    let _ = pipeline.stop_recording();
    set_pipeline_idle(&app, &state.inner());
    wake_handle.resume();
    tracing::info!("dictation cancelled by overlay");
    Ok(())
}

#[tauri::command]
pub async fn stop_dictation(app: AppHandle) -> AppResult<Transcript> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();
    let operation = pipeline.operation_id();

    // Убедимся, что модель whisper загружена.
    if let Some(path) = settings.whisper_model_path.as_deref() {
        pipeline
            .stt()
            .ensure_loaded(std::path::Path::new(path), settings.acceleration.use_gpu())?;
    } else {
        let error_msg =
            "Whisper model is not selected. Download and choose a model in settings.".to_string();
        emit_pipeline_error(&app, &error_msg);
        set_pipeline_idle(&app, &state.inner());
        return Err(AppError::Stt(error_msg));
    }
    if !operation_still_active(&pipeline, operation, "stop_dictation after model load") {
        return Ok(empty_transcript());
    }

    let samples = match pipeline.stop_recording() {
        Ok(s) => s,
        Err(e) => {
            emit_pipeline_error(&app, &e.to_string());
            tracing::error!("stop_dictation: stop_recording FAILED: {e}");
            set_pipeline_idle(&app, &state.inner());
            return Err(e);
        }
    };
    if samples.is_empty() {
        set_pipeline_idle(&app, &state.inner());
        return Ok(empty_transcript());
    }

    // VAD: обрезаем тишину в начале/конце — whisper получит меньше аудио,
    // значит отработает быстрее (ускорение 1.5-2x на типичной записи).
    let samples = crate::vad::trim_silence(&samples);
    if samples.is_empty() {
        tracing::info!("VAD: речь не обнаружена вообще — пропускаем транскрипцию");
        set_pipeline_idle(&app, &state.inner());
        return Ok(empty_transcript());
    }

    // Транскрибируем (CPU-bound — запускаем в spawn_blocking).
    pipeline::set_state(&app, &state.inner(), PipelineState::Transcribing);
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let app_for_err = app.clone();
    let transcript =
        tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
            .await
            .map_err(|e| {
                let _ = app_for_err.emit("error", e.to_string());
                set_pipeline_idle(&app_for_err, &state.inner());
                AppError::Internal(format!("transcribe join: {e}"))
            })?
            .map_err(|e| {
                let _ = app.emit("error", e.to_string());
                set_pipeline_idle(&app, &state.inner());
                e
            })?;

    if !operation_still_active(&pipeline, operation, "stop_dictation after transcription") {
        return Ok(empty_transcript());
    }

    tracing::info!("transcript ready ({} chars)", transcript.text.chars().count());

    // Опциональная AI-обработка.
    let final_text = match settings.ai_mode {
        AiMode::Off => transcript.text.clone(),
        mode => {
            pipeline::set_state(&app, &state.inner(), PipelineState::Processing);
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

    crate::vlog!("dictation final text ready ({} chars)", final_text.chars().count());

    // Вставка текста.
    pipeline::set_state(&app, &state.inner(), PipelineState::Injecting);
    if let Err(e) = crate::injection::inject_text(&final_text, settings.injection_mode) {
        emit_pipeline_error(&app, &e.to_string());
        set_pipeline_idle(&app, &state.inner());
        return Err(e);
    }
    set_pipeline_idle(&app, &state.inner());

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
#[tauri::command]
pub async fn transcribe_test(
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
        pipeline
            .stt()
            .ensure_loaded(std::path::Path::new(path), settings.acceleration.use_gpu())?;
    } else {
        let error_msg =
            "Whisper model is not selected. Download and choose a model in settings.".to_string();
        emit_pipeline_error(&app, &error_msg);
        set_pipeline_idle(&app, &state.inner());
        return Err(AppError::Stt(error_msg));
    }

    // Старт записи.
    tracing::info!(
        "transcribe_test: starting recording (device_id={:?})",
        settings.audio_device_id
    );
    pipeline::set_state(&app, &state.inner(), PipelineState::Listening);
    if let Err(e) = pipeline.start_recording(settings.audio_device_id.as_deref()) {
        emit_pipeline_error(&app, &e.to_string());
        tracing::error!("transcribe_test: start_recording FAILED: {e}");
        set_pipeline_idle(&app, &state.inner());
        return Err(e);
    }
    let operation = pipeline.operation_id();
    tracing::info!(
        "transcribe_test: recording started, sleeping {} ms",
        duration_ms
    );

    // Ждём указанную длительность.
    let dur = std::time::Duration::from_millis(duration_ms.max(500).min(30_000));
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
            set_pipeline_idle(&app, &state.inner());
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
        set_pipeline_idle(&app, &state.inner());
        return Err(AppError::Audio(
            "записано слишком мало аудио — проверьте, что микрофон работает и не занят другим приложением".into(),
        ));
    }

    // Транскрипция (CPU-bound).
    pipeline::set_state(&app, &state.inner(), PipelineState::Transcribing);
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let app_for_err = app.clone();
    let transcript =
        tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
            .await
            .map_err(|e| {
                let _ = app_for_err.emit("error", e.to_string());
                set_pipeline_idle(&app_for_err, &state.inner());
                AppError::Internal(format!("transcribe join: {e}"))
            })?
            .map_err(|e| {
                let _ = app.emit("error", e.to_string());
                set_pipeline_idle(&app, &state.inner());
                e
            })?;

    if !operation_still_active(&pipeline, operation, "transcribe_test after transcription") {
        return Ok(empty_transcript());
    }

    tracing::info!("test transcript ready ({} chars)", transcript.text.chars().count());

    // Опциональная AI-обработка — но на ошибке не падаем.
    let final_text = match settings.ai_mode {
        AiMode::Off => transcript.text.clone(),
        mode => {
            pipeline::set_state(&app, &state.inner(), PipelineState::Processing);
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
        pipeline::set_state(&app, &state.inner(), PipelineState::Injecting);
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

    set_pipeline_idle(&app, &state.inner());

    Ok(Transcript {
        text: final_text,
        detected_language: transcript.detected_language,
        transcribe_secs: transcript.transcribe_secs,
        audio_secs: transcript.audio_secs,
        device: transcript.device,
    })
}

// ====== Аудио ======

#[tauri::command]
pub fn list_audio_devices() -> AppResult<Vec<DeviceInfo>> {
    AudioCapture::list_input_devices()
}

// ====== Whisper-модели ======

#[tauri::command]
pub fn list_whisper_models() -> AppResult<Vec<WhisperModelInfo>> {
    let dir = state::models_dir()?;
    let mut out = Vec::new();
    for size in [
        WhisperModelSize::Tiny,
        WhisperModelSize::Base,
        WhisperModelSize::Small,
        WhisperModelSize::Medium,
        WhisperModelSize::Large,
    ] {
        let filename = size.filename();
        let path = dir.join(filename);
        let (local_path, bytes) = if path.exists() {
            let meta = std::fs::metadata(&path).ok();
            (
                Some(path.to_string_lossy().to_string()),
                meta.map(|m| m.len()),
            )
        } else {
            (None, Some(size.approx_bytes()))
        };
        out.push(WhisperModelInfo {
            filename: filename.to_string(),
            size,
            local_path,
            bytes,
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn download_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    let model_size: WhisperModelSize =
        serde_json::from_value(serde_json::Value::String(size.clone()))
            .map_err(|_| AppError::Config(format!("неизвестный размер модели: {size}")))?;
    let url = model_size.url().to_string();
    let target: PathBuf = state::models_dir()?.join(model_size.filename());

    tracing::info!("downloading {} -> {}", url, target.display());

    let app_clone = app.clone();
    let filename = model_size.filename().to_string();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| AppError::Config(e.to_string()))?;
        let mut resp = client
            .get(&url)
            .send()
            .map_err(|e| AppError::Config(format!("GET {url}: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::Config(format!(
                "HTTP {} при скачивании модели",
                resp.status()
            )));
        }
        let mut file = std::fs::File::create(&target)?;
        resp.copy_to(&mut file)
            .map_err(|e| AppError::Config(format!("copy_to: {e}")))?;
        tracing::info!("model saved: {}", target.display());
        let _ = app_clone.emit("model-downloaded", filename);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(())
}

// ====== KWS-модель (sherpa-onnx wake word) ======

const KWS_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2";
const KWS_MODEL_DIR: &str = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01";
const KWS_ARCHIVE: &str = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2";

/// Проверяет, скачана ли KWS-модель для sherpa-onnx backend.
#[tauri::command]
pub fn is_kws_model_downloaded() -> AppResult<bool> {
    let dir = state::app_data_dir()?
        .join("kws-models")
        .join(KWS_MODEL_DIR);
    let encoder = dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let decoder = dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let joiner = dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let tokens = dir.join("tokens.txt");
    Ok(encoder.is_file() && decoder.is_file() && joiner.is_file() && tokens.is_file())
}

/// Скачивает и распаковывает англоязычную KWS-модель sherpa-onnx.
#[tauri::command]
pub async fn download_kws_model(app: AppHandle) -> AppResult<()> {
    let base_dir = state::app_data_dir()?.join("kws-models");
    std::fs::create_dir_all(&base_dir)?;
    let archive_path = base_dir.join(KWS_ARCHIVE);
    let url = KWS_MODEL_URL.to_string();

    tracing::info!(
        "downloading KWS model {} -> {}",
        url,
        archive_path.display()
    );
    let app_clone = app.clone();

    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| AppError::Config(e.to_string()))?;
        let mut resp = client
            .get(&url)
            .send()
            .map_err(|e| AppError::Config(format!("GET {url}: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::Config(format!(
                "HTTP {} при скачивании KWS-модели",
                resp.status()
            )));
        }
        let mut file = std::fs::File::create(&archive_path)?;
        resp.copy_to(&mut file)
            .map_err(|e| AppError::Config(format!("copy_to: {e}")))?;
        drop(file);

        tracing::info!("extracting KWS model to {}", base_dir.display());
        let file = std::fs::File::open(&archive_path)?;
        let decompress = bzip2::read::BzDecoder::new(file);
        let mut archive = tar::Archive::new(decompress);
        archive
            .unpack(&base_dir)
            .map_err(|e| AppError::Config(format!("unpack: {e}")))?;

        let _ = std::fs::remove_file(&archive_path);
        tracing::info!("KWS model ready");
        let _ = app_clone.emit("kws-model-downloaded", true);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(())
}

#[tauri::command]
pub fn set_whisper_model(state: State<'_, AppState>, path: String) -> AppResult<()> {
    let mut settings = state.settings();
    settings.whisper_model_path = Some(path);
    state.set_settings(settings.clone());
    state::save_settings(&settings)?;
    Ok(())
}

// ====== LLM ======

#[tauri::command]
pub async fn test_llm_connection(state: State<'_, AppState>) -> AppResult<String> {
    let s = state.settings();
    let client = LlmClient::from_settings(&s);
    let model_id = client.test_connection().await?;
    Ok(format!("LLM активен, модель: {model_id}"))
}

#[tauri::command]
pub async fn list_llm_models(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let s = state.settings();
    let client = LlmClient::from_settings(&s);
    client.list_models().await
}

// ====== Настройки ======

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<()> {
    let old_settings = state.settings();
    let shortcuts_changed = old_settings.hotkey != settings.hotkey
        || old_settings.command_hotkey != settings.command_hotkey;
    let wake_settings_changed = old_settings.wake_word != settings.wake_word
        || old_settings.wake_word_model != settings.wake_word_model
        || old_settings.wake_backend != settings.wake_backend
        || (old_settings.wake_word_threshold - settings.wake_word_threshold).abs() > f32::EPSILON
        || (old_settings.wake_word_sensitivity - settings.wake_word_sensitivity).abs()
            > f32::EPSILON
        || (old_settings.wake_word_vad_threshold - settings.wake_word_vad_threshold).abs()
            > f32::EPSILON;

    state::save_settings(&settings)?;
    state.set_settings(settings.clone());
    crate::verbose::set_verbose(settings.verbose_logging);
    let _ = app.emit("settings-changed", settings.clone());
    tracing::info!(
        "settings saved: model={:?}, lang={}",
        settings.whisper_model_path,
        settings.language
    );

    // Если wake word уже работает и изменились его настройки — перезапускаем.
    if settings.wake_word_enabled && wake_settings_changed {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::restart_wake_word(&app_clone).await {
                tracing::error!("wake word restart after settings change failed: {e}");
                let _ =
                    app_clone.emit("error", format!("Wake word: не удалось перезапустить: {e}"));
            }
        });
    }

    // Перерегистрируем глобальные шорткаты, если изменились hotkey/command_hotkey.
    if shortcuts_changed {
        if let Err(e) = crate::register_all_shortcuts(&app, &settings) {
            // Восстанавливаем старые значения hotkey.
            let mut reverted = settings.clone();
            reverted.hotkey = old_settings.hotkey.clone();
            reverted.command_hotkey = old_settings.command_hotkey.clone();
            let _ = state::save_settings(&reverted);
            state.set_settings(reverted);
            return Err(AppError::Config(format!(
                "Не удалось зарегистрировать горячие клавиши: {e}. Старые значения восстановлены."
            )));
        }
    }

    Ok(())
}

// ====== Диагностика ======

/// Очищает текущий лог-файл (truncate).
#[tauri::command]
pub fn clear_logs() -> AppResult<()> {
    let log_dir = state::app_data_dir()?.join("logs");
    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)
        .map_err(|e| AppError::Io(e))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    if let Some(target) = entries
        .iter()
        .rev()
        .find(|e| e.file_name().to_string_lossy().starts_with("fono.log"))
    {
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(target.path())
            .map_err(|e| AppError::Io(e))?;
        tracing::info!("log file cleared");
    }
    Ok(())
}

/// Возвращает последние строки файла лога (для отображения в UI при ошибках).
#[tauri::command]
pub fn get_recent_logs(lines: Option<usize>) -> AppResult<String> {
    let n = lines.unwrap_or(80).min(500);
    let log_dir = state::app_data_dir()?.join("logs");
    // Ищем самый свежий fono.log* (rolling appender добавляет дату).
    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)
        .map_err(|e| AppError::Io(e))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    let target = entries
        .iter()
        .rev()
        .find(|e| e.file_name().to_string_lossy().starts_with("fono.log"))
        .ok_or_else(|| AppError::Internal("лог-файл не найден".into()))?;

    let content = std::fs::read_to_string(target.path())?;
    let tail: String = content
        .lines()
        .rev()
        .take(n)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Ok(tail)
}

/// Тест микрофона: записывает `duration_ms` и возвращает пиковый уровень (0..1)
/// и количество сэмплов. Позволяет убедиться, что микрофон живой и не зашумлён.
#[tauri::command]
pub async fn test_microphone(app: AppHandle, duration_ms: u64) -> AppResult<MicTestResult> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();

    pipeline::set_state(&app, &state.inner(), PipelineState::Listening);
    if let Err(e) = pipeline.start_recording(settings.audio_device_id.as_deref()) {
        emit_pipeline_error(&app, &e.to_string());
        tracing::error!("test_microphone: start_recording FAILED: {e}");
        set_pipeline_idle(&app, &state.inner());
        return Err(e);
    }

    let dur = std::time::Duration::from_millis(duration_ms.max(500).min(5_000));
    tokio::time::sleep(dur).await;

    let samples = match pipeline.stop_recording() {
        Ok(s) => s,
        Err(e) => {
            emit_pipeline_error(&app, &e.to_string());
            tracing::error!("test_microphone: stop_recording FAILED: {e}");
            set_pipeline_idle(&app, &state.inner());
            return Err(e);
        }
    };
    set_pipeline_idle(&app, &state.inner());

    if samples.is_empty() {
        emit_pipeline_error(&app, "No input captured. Check microphone and permissions.");
        return Err(AppError::Audio(
            "не получено ни одного сэмпла — микрофон молчит или занят".into(),
        ));
    }

    // Считаем пиковый и RMS уровень.
    let mut peak: i32 = 0;
    let mut sum_sq: i64 = 0;
    for &s in &samples {
        let a = s.unsigned_abs() as i32;
        if a > peak {
            peak = a;
        }
        sum_sq += (s as i64) * (s as i64);
    }
    let rms = ((sum_sq as f64 / samples.len() as f64).sqrt()) as f32;
    let peak_norm = peak as f32 / i16::MAX as f32;
    let rms_norm = rms / i16::MAX as f32;

    Ok(MicTestResult {
        samples: samples.len(),
        duration_ms: (samples.len() as f64 / 16_000.0 * 1000.0) as u64,
        peak: peak_norm,
        rms: rms_norm,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct MicTestResult {
    pub samples: usize,
    pub duration_ms: u64,
    /// Пиковый уровень, 0.0..1.0 (1.0 = clipping).
    pub peak: f32,
    /// Среднеквадратичный уровень, 0.0..1.0.
    pub rms: f32,
}

// ====== Overlay ======

#[tauri::command]
pub fn save_overlay_position(state: State<'_, AppState>, x: i32, y: i32) -> AppResult<()> {
    let mut settings = state.settings();
    settings.overlay_x = Some(x);
    settings.overlay_y = Some(y);
    state.set_settings(settings.clone());
    state::save_settings(&settings)?;
    Ok(())
}

// ====== Wake word ======

/// Возвращает статус wake word детектора.
#[tauri::command]
pub fn get_wake_word_status(wake_handle: State<'_, fono_wake::WakeWordHandle>) -> String {
    format!("{}", wake_handle.status())
}

/// Возвращает runtime-диагностику wake word (уровень сигнала, последний результат).
#[tauri::command]
pub fn get_wake_word_diagnostics(
    wake_handle: State<'_, fono_wake::WakeWordHandle>,
) -> Option<fono_wake::Diagnostics> {
    wake_handle.diagnostics()
}

#[derive(Debug, serde::Serialize)]
pub struct WakeWordTestReport {
    pub detected: bool,
    pub keyword: String,
    pub json: String,
    pub duration_ms: u64,
}

/// Запускает оффлайн-проверку wake word на тестовом WAV из модели.
#[cfg(feature = "sherpa-wake")]
#[tauri::command]
pub async fn test_wake_word_model(app: AppHandle) -> AppResult<WakeWordTestReport> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let config = crate::settings_to_wake_config(&settings)?;
    let wav_path = config.model_dir.join("test_wavs").join("0.wav");
    if !wav_path.is_file() {
        return Err(AppError::Internal(format!(
            "тестовый WAV не найден: {}",
            wav_path.display()
        )));
    }
    // Используем встроенные ключевые слова модели, чтобы проверить саму
    // модель/споттер независимо от пользовательской фразы и микрофона.
    let res = fono_wake::test_with_wav(&config, &wav_path, true)?;
    Ok(WakeWordTestReport {
        detected: res.detected,
        keyword: res.keyword,
        json: res.json,
        duration_ms: res.duration_ms,
    })
}

#[cfg(not(feature = "sherpa-wake"))]
#[tauri::command]
pub async fn test_wake_word_model(_app: AppHandle) -> AppResult<WakeWordTestReport> {
    Err(AppError::Internal(
        "sherpa-wake backend не собран в эту сборку".into(),
    ))
}

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
#[tauri::command]
pub async fn record_wake_word_sample(
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
    pipeline::set_state(&app, &state.inner(), PipelineState::Listening);
    if let Err(error) = pipeline.start_recording(settings.audio_device_id.as_deref()) {
        wake_handle.resume();
        set_pipeline_idle(&app, &state.inner());
        return Err(error);
    }

    let duration = std::time::Duration::from_millis(duration_ms.clamp(1_000, 10_000));
    tokio::time::sleep(duration).await;
    let samples = pipeline.stop_recording();
    set_pipeline_idle(&app, &state.inner());
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
#[tauri::command]
pub async fn recognize_wake_word_sample(app: AppHandle) -> AppResult<WakeWordRecognitionReport> {
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

    let result = match settings.wake_backend {
        fono_wake::WakeWordBackend::WhisperExperimental => {
            tauri::async_runtime::spawn_blocking(move || {
                fono_wake::test_whisper_with_samples(&config, &samples)
            })
            .await
            .map_err(|error| AppError::Internal(format!("wake test join: {error}")))??
        }
        fono_wake::WakeWordBackend::SherpaOnnx => {
            let wav_path = state::app_data_dir()?.join("wake-word-test.wav");
            write_pcm16_wav(&wav_path, &samples, 16_000)?;
            tauri::async_runtime::spawn_blocking(move || {
                fono_wake::test_with_wav(&config, &wav_path, false)
            })
            .await
            .map_err(|error| AppError::Internal(format!("wake test join: {error}")))??
        }
        _ => {
            return Err(AppError::Internal(
                "тест записи поддерживается для Whisper и Sherpa-ONNX".into(),
            ));
        }
    };

    Ok(WakeWordRecognitionReport {
        backend,
        detected: result.detected,
        recognized: result.keyword,
        json: result.json,
        audio_duration_ms,
        processing_ms: started.elapsed().as_millis() as u64,
    })
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

fn write_pcm16_wav(path: &std::path::Path, samples: &[i16], sample_rate: u32) -> AppResult<()> {
    let data_len = (samples.len() * std::mem::size_of::<i16>()) as u32;
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

/// Включает wake word детектор.
#[tauri::command]
pub async fn enable_wake_word(app: AppHandle) -> AppResult<()> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();
    let settings = state.settings();

    let app_clone = app.clone();
    wake_handle.set_callback(move |event| match event {
        fono_wake::WakeWordEvent::Detected { phrase, pre_roll } => {
            tracing::info!("wake word triggered: {phrase}");
            let _ = app_clone.emit("wake-word-detected", &phrase);
            let h = app_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::run_dictation_after_wake(&h, pre_roll).await {
                    tracing::error!("dictation after wake failed: {e}");
                    let _ = h.emit("error", e.to_string());
                }
            });
        }
        fono_wake::WakeWordEvent::Error { message } => {
            let _ = app_clone.emit("error", &message);
        }
        fono_wake::WakeWordEvent::Listening => {
            let _ = app_clone.emit("wake-word-status", "listening");
        }
        fono_wake::WakeWordEvent::Paused => {
            let _ = app_clone.emit("wake-word-status", "paused");
        }
        fono_wake::WakeWordEvent::ModelLoading => {
            let _ = app_clone.emit("wake-word-status", "loading");
        }
        fono_wake::WakeWordEvent::MissingModel { path } => {
            tracing::warn!("wake word model missing: {path}");
            let _ = app_clone.emit("wake-word-status", "missing_model");
            let _ = app_clone.emit("error", format!("Wake word: модель не найдена: {path}"));
        }
    });

    let mut config = crate::settings_to_wake_config(&settings)?;
    // Включаем независимо от того, что сейчас записано в настройках:
    // UI сохраняет настройки ДО вызова этой команды, поэтому флаг там ещё false.
    config.enabled = true;
    wake_handle.update_config(config)?;

    // Сохраняем в настройках.
    let mut s = settings;
    s.wake_word_enabled = true;
    crate::state::save_settings(&s)?;
    state.set_settings(s);

    tracing::info!("wake word enabled");
    Ok(())
}

/// Выключает wake word детектор.
#[tauri::command]
pub async fn disable_wake_word(app: AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();

    wake_handle.stop();

    let mut s = state.settings();
    s.wake_word_enabled = false;
    crate::state::save_settings(&s)?;
    state.set_settings(s);

    tracing::info!("wake word disabled");
    Ok(())
}
