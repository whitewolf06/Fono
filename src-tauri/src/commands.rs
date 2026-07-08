//! Tauri IPC команды — мост между фронтендом и Rust-ядром.
//!
//! Каждая команда доступна из JS через `invoke('<name>', { args })`.
//! Список команд см. в `docs/architecture.md` → `commands.rs`.
//!
//! В async-командах мы используем `AppHandle::state::<T>()` вместо
//! `State<'_, T>`, чтобы не удерживать borrow через `.await`.

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

fn emit_pipeline_error(app: &AppHandle, message: &str) {
    let _ = app.emit("error", message);
}

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
pub async fn stop_dictation(app: AppHandle) -> AppResult<Transcript> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();

    // Убедимся, что модель whisper загружена.
    if let Some(path) = settings.whisper_model_path.as_deref() {
        pipeline
            .stt()
            .ensure_loaded(std::path::Path::new(path), settings.use_gpu)?;
    } else {
        let error_msg = "Whisper model is not selected. Download and choose a model in settings.".to_string();
        emit_pipeline_error(&app, &error_msg);
        set_pipeline_idle(&app, &state.inner());
        return Err(AppError::Stt(error_msg));
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
        return Ok(Transcript {
            text: String::new(),
            detected_language: None,
            transcribe_secs: None,
            audio_secs: None,
            device: None,
        });
    }

    // VAD: обрезаем тишину в начале/конце — whisper получит меньше аудио,
    // значит отработает быстрее (ускорение 1.5-2x на типичной записи).
    let samples = crate::vad::trim_silence(&samples);
    if samples.is_empty() {
        tracing::info!("VAD: речь не обнаружена вообще — пропускаем транскрипцию");
        set_pipeline_idle(&app, &state.inner());
        return Ok(Transcript {
            text: String::new(),
            detected_language: None,
            transcribe_secs: None,
            audio_secs: None,
            device: None,
        });
    }

    // Транскрибируем (CPU-bound — запускаем в spawn_blocking).
    pipeline::set_state(&app, &state.inner(), PipelineState::Transcribing);
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let app_for_err = app.clone();
    let transcript = tauri::async_runtime::spawn_blocking(move || {
        stt.transcribe(&samples, &language)
    })
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

    tracing::info!("transcript: {:?}", transcript.text);

    // Опциональная AI-обработка.
    let final_text = match settings.ai_mode {
        AiMode::Off => transcript.text.clone(),
        mode => {
            pipeline::set_state(&app, &state.inner(), PipelineState::Processing);
            let client = LlmClient::new(
                settings.llm_base_url.clone(),
                settings.llm_model.clone(),
            );
            match client.process(&transcript.text, mode, settings.clean_prompt.as_deref()).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("LLM failed ({e}) — returning raw transcript");
                    let _ = app.emit("error", format!("LLM: {e}"));
                    transcript.text.clone()
                }
            }
        }
    };

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
            .ensure_loaded(std::path::Path::new(path), settings.use_gpu)?;
    } else {
        let error_msg = "Whisper model is not selected. Download and choose a model in settings.".to_string();
        emit_pipeline_error(&app, &error_msg);
        set_pipeline_idle(&app, &state.inner());
        return Err(AppError::Stt(error_msg));
    }

    // Старт записи.
    tracing::info!("transcribe_test: starting recording (device_id={:?})", settings.audio_device_id);
    pipeline::set_state(&app, &state.inner(), PipelineState::Listening);
    if let Err(e) = pipeline.start_recording(settings.audio_device_id.as_deref()) {
        emit_pipeline_error(&app, &e.to_string());
        tracing::error!("transcribe_test: start_recording FAILED: {e}");
        set_pipeline_idle(&app, &state.inner());
        return Err(e);
    }
    tracing::info!("transcribe_test: recording started, sleeping {} ms", duration_ms);

    // Ждём указанную длительность.
    let dur = std::time::Duration::from_millis(duration_ms.max(500).min(30_000));
    tokio::time::sleep(dur).await;

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
    tracing::info!("captured {} samples (~{:.1}s @ 16kHz)", sample_count, sample_count as f32 / 16_000.0);

    if sample_count < 1600 {
        emit_pipeline_error(&app, "Test recording is too short or too quiet. Please speak closer and longer.");
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
    let transcript = tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
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

    tracing::info!("test transcript: {:?}", transcript.text);

    // Опциональная AI-обработка — но на ошибке не падаем.
    let final_text = match settings.ai_mode {
        AiMode::Off => transcript.text.clone(),
        mode => {
            pipeline::set_state(&app, &state.inner(), PipelineState::Processing);
            let client = LlmClient::new(
                settings.llm_base_url.clone(),
                settings.llm_model.clone(),
            );
            match client.process(&transcript.text, mode, settings.clean_prompt.as_deref()).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("LLM failed ({e}) — returning raw transcript");
                    transcript.text.clone()
                }
            }
        }
    };

    // Этап 2: текст-инъекция в активное окно через SendInput.
    if inject && !final_text.is_empty() {
        pipeline::set_state(&app, &state.inner(), PipelineState::Injecting);
        match crate::injection::inject_text(&final_text, settings.injection_mode) {
            Ok(()) => tracing::info!("injected {} chars into active window", final_text.chars().count()),
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
    let client = LlmClient::new(s.llm_base_url.clone(), s.llm_model.clone());
    let model_id = client.test_connection().await?;
    Ok(format!("LM Studio активен, модель: {model_id}"))
}

#[tauri::command]
pub async fn list_llm_models(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let s = state.settings();
    let client = LlmClient::new(s.llm_base_url.clone(), None);
    client.list_models().await
}

// ====== Настройки ======

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> AppResult<()> {
    let old_settings = state.settings();
    let hotkey_changed = old_settings.hotkey != settings.hotkey;

    state::save_settings(&settings)?;
    state.set_settings(settings.clone());
    tracing::info!("settings saved: model={:?}, lang={}", settings.whisper_model_path, settings.language);

    // Перерегистрируем глобальную горячую клавишу push-to-talk, если она изменилась.
    if hotkey_changed {
        if let Err(e) = crate::register_push_to_talk(&app, &settings.hotkey) {
            // В случае ошибки восстанавливаем старый hotkey в настройках.
            let mut reverted = settings.clone();
            reverted.hotkey = old_settings.hotkey.clone();
            let _ = state::save_settings(&reverted);
            state.set_settings(reverted);
            return Err(AppError::Config(format!(
                "Не удалось зарегистрировать горячую клавишу '{}': {e}. Старое значение восстановлено.",
                settings.hotkey
            )));
        }
    }

    Ok(())
}

// ====== Диагностика ======

/// Возвращает последние строки файла лога (для отображения в UI при ошибках).
#[tauri::command]
pub fn get_recent_logs(lines: Option<usize>) -> AppResult<String> {
    let n = lines.unwrap_or(80).min(500);
    let log_dir = state::app_data_dir()?.join("logs");
    // Ищем самый свежий whisperclone.log* (rolling appender добавляет дату).
    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)
        .map_err(|e| AppError::Io(e))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    let target = entries
        .iter()
        .rev()
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("whisperclone.log")
        })
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
pub async fn test_microphone(
    app: AppHandle,
    duration_ms: u64,
) -> AppResult<MicTestResult> {
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
pub fn save_overlay_position(
    state: State<'_, AppState>,
    x: i32,
    y: i32,
) -> AppResult<()> {
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
pub fn get_wake_word_status(
    detector: State<'_, crate::wakeword::WakeWordDetector>,
) -> String {
    format!("{:?}", detector.status())
}

/// Включает wake word детектор.
#[tauri::command]
pub async fn enable_wake_word(app: AppHandle) -> AppResult<()> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let detector = app.state::<crate::wakeword::WakeWordDetector>();
    let settings = state.settings();

    // Загружаем base-модель для wake word.
    let models_dir = crate::state::models_dir()?;
    let base_path = models_dir.join("ggml-base.bin");
    if !base_path.exists() {
        return Err(AppError::Config(
            "Модель base не скачана. Скачайте её в разделе «Модель распознавания».".into(),
        ));
    }
    pipeline.stt().ensure_loaded(&base_path, settings.use_gpu)?;

    detector.set_phrase(settings.wake_word.clone());

    let stt = pipeline.stt().clone();
    let device_id = settings.audio_device_id.clone();
    let app_clone = app.clone();

    detector.start(stt, device_id, move |event| {
        match event {
            crate::wakeword::WakeEvent::Detected { transcription } => {
                tracing::info!("wake word triggered: {:?}", transcription);
                let _ = app_clone.emit("wake-word-detected", &transcription);
                let h = app_clone.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::run_dictation_after_wake(&h).await {
                        tracing::error!("dictation after wake failed: {e}");
                        let _ = h.emit("error", e.to_string());
                    }
                });
            }
            crate::wakeword::WakeEvent::Error(msg) => {
                let _ = app_clone.emit("error", &msg);
            }
            crate::wakeword::WakeEvent::Status(s) => {
                let _ = app_clone.emit("wake-word-status", format!("{:?}", s));
            }
        }
    })?;

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
pub async fn disable_wake_word(
    app: AppHandle,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    let detector = app.state::<crate::wakeword::WakeWordDetector>();
    detector.stop();

    let mut s = state.settings();
    s.wake_word_enabled = false;
    crate::state::save_settings(&s)?;
    state.set_settings(s);

    tracing::info!("wake word disabled");
    Ok(())
}


