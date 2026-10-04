use crate::{
    error::{AppError, AppResult},
    types::Settings,
};

pub(super) fn validate(active: bool, previous: &Settings, next: &Settings) -> AppResult<()> {
    let capture_changed = previous.dictation_mode != next.dictation_mode
        || previous.audio_device_id != next.audio_device_id
        || previous.whisper_model_path != next.whisper_model_path
        || previous.acceleration != next.acceleration
        || previous.gpu_model_residency != next.gpu_model_residency
        || previous.hotkey != next.hotkey
        || previous.hotkey_mode != next.hotkey_mode
        || previous.command_hotkey != next.command_hotkey
        || previous.ai_mode != next.ai_mode
        || previous.processing_workflow != next.processing_workflow
        || previous.processing_preset != next.processing_preset
        || previous.processing_target_language != next.processing_target_language
        || previous.processing_translation_enabled != next.processing_translation_enabled
        || previous.clean_prompt != next.clean_prompt
        || previous.processing_prompts != next.processing_prompts
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
    fn gpu_residency_changes_apply_between_capture_sessions() {
        let previous = Settings::default();
        let next = Settings {
            gpu_model_residency: crate::types::GpuModelResidency::Adaptive,
            ..previous.clone()
        };
        assert!(validate(true, &previous, &next).is_err());
        assert!(validate(false, &previous, &next).is_ok());
    }
    #[test]
    fn only_actual_prompt_changes_block_active_capture() {
        let previous = Settings::default();
        let roundtrip: Settings =
            serde_json::from_value(serde_json::to_value(&previous).unwrap()).unwrap();
        assert!(validate(true, &previous, &roundtrip).is_ok());
        let mut next = roundtrip;
        next.processing_prompts.task.custom_prompt = "Свой промпт".into();
        assert!(validate(true, &previous, &next).is_err());
        assert!(validate(false, &previous, &next).is_ok());
    }
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
