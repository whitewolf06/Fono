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
        || previous.hotkey_mode != next.hotkey_mode
        || previous.command_hotkey != next.command_hotkey
        || previous.ai_mode != next.ai_mode
        || previous.processing_workflow != next.processing_workflow
        || previous.processing_preset != next.processing_preset
        || previous.processing_target_language != next.processing_target_language
        || previous.clean_prompt != next.clean_prompt
        || previous.llm_profiles != next.llm_profiles
        || previous.text_correction_llm != next.text_correction_llm;
    if active && capture_changed {
        return Err(AppError::Busy("Сначала завершите диктовку: настройки записи, горячих клавиш и обработки текста применяются между сеансами".into()));
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

    #[test]
    fn hotkey_behavior_changes_only_between_operations() {
        let previous = Settings::default();
        let next = Settings {
            hotkey_mode: crate::types::HotkeyMode::Toggle,
            ..previous.clone()
        };
        assert!(validate(true, &previous, &next).is_err());
        assert!(validate(false, &previous, &next).is_ok());
    }

    #[test]
    fn changing_processing_workflow_cannot_hide_pending_result() {
        let previous = Settings::default();
        let next = Settings {
            processing_workflow: crate::types::ProcessingWorkflow::Manual,
            ..previous.clone()
        };
        assert!(validate(true, &previous, &next).is_err());
        assert!(validate(false, &previous, &next).is_ok());
    }
}
