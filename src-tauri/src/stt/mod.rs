//! Speech-to-Text через whisper.cpp (биндинг [`whisper_rs`] v0.16).
//!
//! Загружает GGML-модель один раз и переиспользует её для всех транскрипций.
//! WhisperContext — потокобезопасен (Arc внутри), но каждый вызов `full`
//! создаёт отдельный WhisperState, поэтому параллельные транскрипции безопасны.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use whisper_rs::{
    SamplingStrategy, WhisperContext, WhisperContextParameters,
};
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::types::Transcript;

pub struct SttEngine {
    ctx: Mutex<Option<Arc<WhisperContext>>>,
    loaded_path: Mutex<Option<String>>,
    use_gpu: AtomicBool,
}

impl SttEngine {
    pub fn new() -> Self {
        Self {
            ctx: Mutex::new(None),
            loaded_path: Mutex::new(None),
            use_gpu: AtomicBool::new(false),
        }
    }

    /// Загружает модель (.bin) если ещё не загружена / путь изменился.
    /// `use_gpu` включает CUDA/Vulkan backend whisper.cpp, если приложение
    /// собрано с соответствующей feature.
    pub fn ensure_loaded(&self, model_path: &Path, use_gpu: bool) -> AppResult<()> {
        let path_str = model_path.to_string_lossy().to_string();
        let already = self.loaded_path.lock().clone();
        if already.as_deref() == Some(path_str.as_str()) {
            return Ok(());
        }
        if !model_path.exists() {
            return Err(AppError::Stt(format!(
                "файл модели не найден: {}",
                model_path.display()
            )));
        }

        tracing::info!("loading whisper model: {} (use_gpu={})", model_path.display(), use_gpu);
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        let ctx = WhisperContext::new_with_params(&path_str, params)
            .map_err(|e| AppError::Stt(format!("WhisperContext::new_with_params: {e}")))?;

        *self.ctx.lock() = Some(Arc::new(ctx));
        *self.loaded_path.lock() = Some(path_str);
        self.use_gpu.store(use_gpu, Ordering::Relaxed);
        tracing::info!("whisper model loaded");
        Ok(())
    }

    /// Возвращает имя активного backend с учётом runtime-флага use_gpu.
    pub fn device(&self) -> &'static str {
        let gpu = self.use_gpu.load(Ordering::Relaxed);
        if gpu {
            #[cfg(feature = "cuda")]
            return "CUDA";
            #[cfg(all(feature = "vulkan", not(feature = "cuda")))]
            return "Vulkan";
        }
        "CPU"
    }


    /// Транскрибирует моно-PCM 16 кГц i16 в текст.
    ///
    /// `language` — ISO-код ("ru", "en") или "auto" для автоопределения.
    pub fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        let ctx_arc = self.ctx.lock().clone();
        let ctx = ctx_arc
            .ok_or(AppError::ModelNotLoaded)?;

        // whisper.cpp ожидает f32 в диапазоне [-1.0, 1.0]
        let pcm_f32: Vec<f32> =
            samples.iter().map(|&s| s as f32 / i16::MAX as f32).collect();

        let mut params = whisper_rs::FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        match language {
            "" | "auto" => {
                params.set_language(None);
            }
            lang => {
                params.set_language(Some(lang));
            }
        }
        let n_threads = num_threads();
        params.set_n_threads(n_threads);
        // Критично для GUI: отключаем весь вывод в stderr/stdout — иначе
        // fprintf блокируется в Tauri webview и whisper зависает намертво.
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        // Не тянуть контекст с прошлых вызовов и не плодить сегменты —
        // каждый вызов независимый, как нам и надо.
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_no_timestamps(true);

        // Каждый вызов создаёт свой state — это безопасно для параллельных вызовов.
        let mut state = ctx
            .create_state()
            .map_err(|e| AppError::Stt(format!("create_state: {e}")))?;

        let started = std::time::Instant::now();
        tracing::info!(
            "whisper full start: {} samples (~{:.2}s), lang={}, threads={}",
            samples.len(),
            samples.len() as f32 / 16_000.0,
            if language.is_empty() { "auto" } else { language },
            n_threads
        );
        state
            .full(params, &pcm_f32)
            .map_err(|e| AppError::Stt(format!("full: {e}")))?;
        let elapsed = started.elapsed().as_secs_f32();
        let audio_secs = samples.len() as f32 / 16_000.0;
        let rtf = if audio_secs > 0.0 { elapsed / audio_secs } else { 0.0 };
        tracing::info!(
            "whisper full done in {:.2}s (audio={:.2}s, RTF={:.2}x, device={})",
            elapsed,
            audio_secs,
            rtf,
            self.device()
        );

        let n_segments = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n_segments {
            if let Some(seg) = state.get_segment(i) {
                if let Ok(t) = seg.to_str_lossy() {
                    let trimmed = t.trim();
                    if !trimmed.is_empty() {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        text.push_str(trimmed);
                    }
                }
            }
        }

        let detected_language = if language.is_empty() || language == "auto" {
            // full_lang_id_from_state возвращает c_int, мапим в строку только если авто
            let id = state.full_lang_id_from_state();
            Some(lang_id_to_str(id))
        } else {
            None
        };

        Ok(Transcript {
            text: text.trim().to_string(),
            detected_language,
            transcribe_secs: Some(elapsed),
            audio_secs: Some(audio_secs),
            device: Some(self.device().to_string()),
        })
    }

    pub fn is_loaded(&self) -> bool {
        self.ctx.lock().is_some()
    }
}

impl Default for SttEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn num_threads() -> std::os::raw::c_int {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // Используем до 8 потоков — whisper.cpp хорошо параллелится,
    // но больше 8 не даёт прироста (memory bandwidth bottleneck).
    cpus.clamp(2, 8) as std::os::raw::c_int
}

/// Преобразует numeric language id в ISO-строку.
/// Список не полный — для неизвестных возвращаем "auto".
fn lang_id_to_str(id: i32) -> String {
    // whisper.cpp использует индекс в массиве языков; точно соответствует списку
    // в `whisper.cpp/src/whisper.cpp` → `whisper_lang_string`.
    // Для упрощения возвращаем числовой код — UI всё равно покажет, что язык определён.
    match id {
        0 => "en".to_string(),
        1 => "zh".to_string(),
        2 => "de".to_string(),
        3 => "es".to_string(),
        4 => "ru".to_string(),
        5 => "ko".to_string(),
        6 => "fr".to_string(),
        _ => format!("lang#{id}"),
    }
}
