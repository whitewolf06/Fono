use super::*;

fn input(value: serde_json::Value) -> ProcessingPreviewInput {
    serde_json::from_value(value).unwrap()
}

#[test]
fn preview_uses_draft_choice_without_mutating_saved_settings() {
    let mut settings = Settings::default();
    settings.processing_prompts.formal = ProcessingPromptChoice {
        use_custom: true,
        custom_prompt: "Сохранённый промпт".into(),
    };
    let request = input(serde_json::json!({
        "text": "Как быстро мы можем закончить задачу?", "preset":"formal", "targetLanguage":"en",
        "promptOverride":{"use_custom":false,"custom_prompt":"Черновик другого промпта"}
    }));
    request.validate().unwrap();
    assert!(!request.choice(&settings).unwrap().use_custom);
    assert_eq!(request.target_language, Some(TranslationLanguage::En));
    assert_eq!(
        settings.processing_prompts.formal.custom_prompt,
        "Сохранённый промпт"
    );
    assert!(settings.processing_prompts.formal.use_custom);
}

#[test]
fn preview_without_override_uses_selected_preset_only() {
    let mut settings = Settings::default();
    settings.processing_prompts.task.custom_prompt = "Особая постановка задачи".into();
    let request = input(serde_json::json!({"text":"Проверь сервис", "preset":"task"}));
    assert_eq!(
        request.choice(&settings).unwrap().custom_prompt,
        "Особая постановка задачи"
    );
    assert_eq!(request.target_language, None);
}

#[test]
fn preview_rejects_empty_text_invalid_custom_and_raw_override_before_network() {
    for value in [
        serde_json::json!({"text":" \n ","preset":"clean"}),
        serde_json::json!({"text":"text","preset":"clean","promptOverride":{"use_custom":true,"custom_prompt":" "}}),
        serde_json::json!({"text":"text","preset":"raw","promptOverride":{"use_custom":false,"custom_prompt":""}}),
        serde_json::json!({"text":"x".repeat(MAX_PROCESSING_TEXT_BYTES + 1),"preset":"clean"}),
    ] {
        assert!(input(value).validate().is_err());
    }
}

#[test]
fn catalog_is_the_same_builtin_text_used_by_native_processing() {
    let catalog = get_processing_prompt_catalog();
    assert_eq!(catalog.presets.len(), 4);
    for descriptor in catalog.presets {
        assert_eq!(
            descriptor.default_prompt,
            presets::default_prompt(descriptor.preset)
        );
        assert!(!descriptor.default_prompt.is_empty());
    }
    assert_eq!(catalog.max_prompt_chars, MAX_PROCESSING_PROMPT_CHARS);
}
