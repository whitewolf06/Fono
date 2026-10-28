//! Offline replay helpers for the retained streaming research backend.

#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeWordTestResult {
    pub detected: bool,
    pub keyword: String,
    pub json: String,
    pub samples: usize,
    pub duration_ms: u64,
}

#[cfg(feature = "sherpa-wake")]
pub fn test_with_wav(
    config: &crate::WakeWordConfig,
    wav_path: &std::path::Path,
    use_builtin_keywords: bool,
) -> crate::WakeWordResult<WakeWordTestResult> {
    // The old third argument remains source-compatible, but the retired KWS
    // model and its bundled keyword vocabulary are no longer available.
    if use_builtin_keywords {
        return Err(crate::WakeWordError::BackendNotCompiled(config.backend));
    }
    crate::validate_config(config)?;
    let report = crate::replay::replay_wav(config, wav_path)?;
    Ok(WakeWordTestResult {
        detected: report.detections > 0,
        keyword: if report.detections > 0 {
            config.phrase.clone()
        } else {
            String::new()
        },
        json: serde_json::to_string(&report).unwrap_or_default(),
        samples: report.audio_samples,
        duration_ms: report.audio_duration_ms,
    })
}

#[cfg(all(test, feature = "sherpa-wake"))]
mod tests {
    use super::*;

    #[test]
    fn retired_kws_test_rejects_before_reading_any_model_or_wav() {
        let config = crate::WakeWordConfig::default();
        let path = std::path::Path::new("missing.wav");
        assert!(matches!(
            test_with_wav(&config, path, true),
            Err(crate::WakeWordError::BackendNotCompiled(_))
        ));
        assert!(matches!(
            test_with_wav(&config, path, false),
            Err(crate::WakeWordError::BackendNotCompiled(_))
        ));
    }
}
