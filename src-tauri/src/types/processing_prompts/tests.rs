use super::*;

#[test]
fn legacy_clean_instruction_is_preserved_and_migration_is_idempotent() {
    let mut settings: Settings = serde_json::from_value(serde_json::json!({
        "clean_prompt": "Сохрани названия функций без изменения"
    }))
    .unwrap();
    assert!(settings.migrate_processing_prompts());
    assert!(settings.processing_prompts.clean.use_custom);
    assert_eq!(
        settings.processing_prompts.clean.custom_prompt,
        "Сохрани названия функций без изменения"
    );
    assert!(!settings.migrate_processing_prompts());
    let saved = serde_json::to_value(&settings).unwrap();
    assert!(saved["processing_prompts"]["clean"]["use_custom"]
        .as_bool()
        .unwrap());
    assert!(saved["processing_prompts"]
        .get("clean_configured")
        .is_none());
}

#[test]
fn explicit_standard_choice_does_not_resurrect_legacy_custom_prompt() {
    let mut settings: Settings = serde_json::from_value(serde_json::json!({
        "clean_prompt": "Старая инструкция",
        "processing_prompts": {
            "clean": {"use_custom": false, "custom_prompt": "Мой черновик"},
            "formal": {"use_custom": true, "custom_prompt": "Только деловой тон"}
        }
    }))
    .unwrap();
    settings.migrate_processing_prompts();
    assert!(!settings.processing_prompts.clean.use_custom);
    assert_eq!(
        settings.processing_prompts.clean.custom_prompt,
        "Мой черновик"
    );
    assert!(settings.clean_prompt.is_none());
    assert!(settings.processing_prompts.formal.use_custom);
    let mut restored: Settings =
        serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
    assert!(!restored.migrate_processing_prompts());
    assert!(restored.clean_prompt.is_none());
}

#[test]
fn disabled_custom_draft_is_retained_but_enabled_empty_or_oversized_is_rejected() {
    assert!(ProcessingPromptChoice::default().validate().is_ok());
    let mut choice = ProcessingPromptChoice {
        use_custom: true,
        custom_prompt: " \n ".into(),
    };
    assert!(choice.validate().is_err());
    choice.use_custom = false;
    assert!(choice.validate().is_ok());
    choice.custom_prompt = "я".repeat(MAX_PROCESSING_PROMPT_CHARS);
    assert!(choice.validate().is_ok());
    choice.custom_prompt.push('я');
    assert!(choice.validate().is_err());
}

#[test]
fn saved_translation_preferences_keep_legacy_enabled_and_can_be_disabled_without_losing_language() {
    let legacy: Settings =
        serde_json::from_value(serde_json::json!({"processing_target_language":"en"})).unwrap();
    assert!(legacy.processing_translation_enabled);
    assert_eq!(
        legacy.processing_target_language,
        Some(crate::types::TranslationLanguage::En)
    );
    let current: Settings = serde_json::from_value(serde_json::json!({"processing_target_language":"fr", "processing_translation_enabled":false})).unwrap();
    assert!(!current.processing_translation_enabled);
    assert_eq!(
        current.processing_target_language,
        Some(crate::types::TranslationLanguage::Fr)
    );
}
