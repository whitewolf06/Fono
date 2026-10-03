use crate::event::capabilities_for_backend;
use crate::{backend, WakeWordBackend, WakeWordConfig, WakeWordError, WakeWordResult};
pub fn validate_config(config: &WakeWordConfig) -> WakeWordResult<()> {
    if config.backend == WakeWordBackend::Disabled {
        return Ok(());
    }
    if config.sample_rate == 0 || !config.threshold.is_finite() || !config.sensitivity.is_finite() {
        return Err(WakeWordError::Backend(
            "invalid wake audio rate or threshold".into(),
        ));
    }
    if matches!(
        config.backend,
        WakeWordBackend::SherpaStreamingRu | WakeWordBackend::SherpaStreamingEn
    ) {
        let normalized = backend::phrase_matcher::normalize(&config.phrase);
        if !(1..=4).contains(&normalized.split_whitespace().count()) {
            return Err(WakeWordError::Backend(
                "Фраза пробуждения должна содержать от одного до четырёх слов".into(),
            ));
        }
        if normalized.chars().count() < 3 || normalized.chars().count() > 80 {
            return Err(WakeWordError::Backend(
                "wake phrase must contain 3 to 80 letters".into(),
            ));
        }
        let language_matches =
            normalized
                .chars()
                .filter(|c| c.is_alphabetic())
                .all(|c| match config.backend {
                    WakeWordBackend::SherpaStreamingRu => ('а'..='я').contains(&c),
                    WakeWordBackend::SherpaStreamingEn => c.is_ascii_alphabetic(),
                    _ => true,
                });
        if !language_matches {
            return Err(WakeWordError::Backend(
                "Фраза не соответствует языку: для русского используйте кириллицу, для английского — латиницу".into(),
            ));
        }
    }
    let capabilities = capabilities_for_backend(config.backend);
    if capabilities.supports_custom_phrase {
        return Ok(());
    }
    let requested = config.phrase.trim();
    if capabilities
        .supported_phrases
        .iter()
        .any(|phrase| phrase.eq_ignore_ascii_case(requested))
    {
        return Ok(());
    }
    Err(WakeWordError::Backend(format!(
        "backend {:?} does not support the wake phrase {:?}",
        config.backend, config.phrase
    )))
}

#[cfg(test)]
mod tests {
    use crate::config::WakeWordConfig;
    use crate::event::WakeWordBackend;

    use super::validate_config;

    #[test]
    fn sherpa_rejects_a_phrase_outside_its_bundled_vocabulary() {
        let config = WakeWordConfig {
            backend: WakeWordBackend::SherpaOnnx,
            phrase: "привет фоно".into(),
            ..WakeWordConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn sherpa_accepts_all_bundled_phrases() {
        for phrase in crate::phrases::SHERPA_SUPPORTED_PHRASES {
            let config = WakeWordConfig {
                backend: WakeWordBackend::SherpaOnnx,
                phrase: phrase.into(),
                ..WakeWordConfig::default()
            };

            assert!(
                validate_config(&config).is_ok(),
                "{phrase} must be accepted"
            );
        }
    }

    #[test]
    fn whisper_accepts_a_custom_phrase() {
        let config = WakeWordConfig {
            backend: WakeWordBackend::WhisperExperimental,
            phrase: "привет фоно".into(),
            ..WakeWordConfig::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn streaming_phrase_checks_words_and_selected_language() {
        for (backend, phrase, valid) in [
            (WakeWordBackend::SherpaStreamingRu, "эй фоно", true),
            (
                WakeWordBackend::SherpaStreamingRu,
                "включи запись моего голоса",
                true,
            ),
            (WakeWordBackend::SherpaStreamingEn, "hello computer", true),
            (WakeWordBackend::SherpaStreamingRu, "эй Fono", false),
            (
                WakeWordBackend::SherpaStreamingEn,
                "привет компьютер",
                false,
            ),
            (
                WakeWordBackend::SherpaStreamingRu,
                "пожалуйста включи запись моего голоса",
                false,
            ),
        ] {
            let config = WakeWordConfig {
                backend,
                phrase: phrase.into(),
                ..Default::default()
            };
            assert_eq!(validate_config(&config).is_ok(), valid, "{phrase}");
        }
    }
}
