use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use whisper_rs::{FullParams, SamplingStrategy};

type NeutralParams = FullParams<'static, 'static>;
static LANGUAGE_PRESETS: OnceLock<Mutex<HashMap<i32, NeutralParams>>> = OnceLock::new();

/// whisper-rs 0.16's set_language uses CString::into_raw without freeing it.
/// Keep a process-lifetime neutral preset per canonical supported language, so
/// that allocation is bounded by Whisper's language table, never by window count.
/// No callback, cancellation guard, context, or prompt tokens enter this cache.
pub(super) fn language_params<'prompt>(
    language: &str,
) -> Result<FullParams<'static, 'prompt>, String> {
    if language.contains('\0') {
        return Err("language contains a null byte".into());
    }
    let id = if language.is_empty() || language == "auto" {
        -1
    } else {
        whisper_rs::get_lang_id(language).ok_or("unsupported Whisper language")?
    };
    let presets = LANGUAGE_PRESETS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut presets = presets
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let preset = presets.entry(id).or_insert_with(|| {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(if id < 0 {
            None
        } else {
            whisper_rs::get_lang_str(id)
        });
        params
    });
    Ok(preset.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_language_is_an_error_without_loading_a_model_or_panicking() {
        assert!(language_params("ru\0en").is_err());
        assert!(language_params("not-a-whisper-language").is_err());
        assert!(language_params("auto").is_ok());
        assert!(language_params("ru").is_ok());
    }
}
