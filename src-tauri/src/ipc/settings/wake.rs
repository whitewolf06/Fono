use crate::types::Settings;

/// Calibration is bound to the phrase, engine, model, microphone and tuning.
/// A renderer cannot carry a validated profile to another audio configuration.
pub(super) fn invalidate_changed(settings: &mut Settings, old: &Settings) -> bool {
    let changed = old.audio_device_id != settings.audio_device_id
        || old.wake_word != settings.wake_word
        || old.wake_word_model != settings.wake_word_model
        || old.wake_backend != settings.wake_backend
        || (old.wake_word_threshold - settings.wake_word_threshold).abs() > f32::EPSILON
        || (old.wake_word_sensitivity - settings.wake_word_sensitivity).abs() > f32::EPSILON
        || (old.wake_word_vad_threshold - settings.wake_word_vad_threshold).abs() > f32::EPSILON;
    if changed {
        settings.wake_calibration_profile = None;
        settings.wake_word_enabled = false;
    } else {
        settings.wake_calibration_profile = old.wake_calibration_profile.clone();
    }
    changed
}
#[derive(Clone, serde::Serialize)]
pub(super) struct WakeConfigurationInvalidated {
    pub reason: &'static str,
    pub message: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn microphone_change_disables_the_old_detector() {
        let old = Settings {
            wake_word_enabled: true,
            ..Settings::default()
        };
        let mut changed = old.clone();
        changed.audio_device_id = Some("new mic".into());
        assert!(invalidate_changed(&mut changed, &old));
        assert!(!changed.wake_word_enabled);
        assert!(changed.wake_calibration_profile.is_none());
    }
    #[test]
    fn nonwake_setting_preserves_activation() {
        let old = Settings {
            wake_word_enabled: true,
            ..Settings::default()
        };
        let mut changed = old.clone();
        changed.autostart = !old.autostart;
        assert!(!invalidate_changed(&mut changed, &old));
        assert!(changed.wake_word_enabled);
    }
}
