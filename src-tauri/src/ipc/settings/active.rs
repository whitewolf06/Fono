use crate::{
    error::{AppError, AppResult},
    types::Settings,
};

pub(super) fn validate(active: bool, previous: &Settings, next: &Settings) -> AppResult<()> {
    let capture_changed = previous.dictation_mode != next.dictation_mode
        || previous.audio_device_id != next.audio_device_id
        || previous.whisper_model_path != next.whisper_model_path
        || previous.acceleration != next.acceleration
        || previous.hotkey != next.hotkey
        || previous.command_hotkey != next.command_hotkey;
    if active && capture_changed {
        return Err(AppError::Busy("Сначала завершите текущую запись: режим, микрофон, модель и горячие клавиши применяются между диктовками".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_mode_cannot_hide_an_active_live_session() {
        let previous = Settings::default();
        let next = Settings {
            dictation_mode: crate::types::DictationMode::Live,
            ..previous.clone()
        };
        assert!(validate(true, &previous, &next).is_err());
        assert!(validate(false, &previous, &next).is_ok());
    }
    #[test]
    fn unrelated_preferences_remain_editable_during_capture() {
        let previous = Settings::default();
        let next = Settings {
            autostart: !previous.autostart,
            ..previous.clone()
        };
        assert!(validate(true, &previous, &next).is_ok());
    }
}
