use super::*;
use crate::application::dictation::workflow::{
    state::{Claim, Record},
    PendingAction, PendingDictation, PendingRequest,
};
use crate::types::AiMode;
use fono_core::OperationSource;

fn request(id: Option<u64>) -> OverlayProcessingChoiceRequest {
    OverlayProcessingChoiceRequest {
        session_id: id,
        preset: TextPreset::Formal,
        target_language: Some(TranslationLanguage::En),
        processing_enabled: None,
        translation_enabled: None,
    }
}
fn pending() -> PendingDictation {
    PendingDictation {
        session_id: 5,
        phase: PendingPhase::AwaitingAction,
        original_text: "test".into(),
        result_text: None,
        created_at: "test".into(),
        preset: TextPreset::Clean,
        target_language: None,
        processing_enabled: true,
        translation_enabled: true,
        source: OperationSource::Hotkey,
        error: None,
        insertion_blocked: false,
        copy_only: false,
    }
}

#[test]
fn failed_save_changes_neither_capture_nor_pending_snapshot() {
    let mut session = Settings::default();
    let mut pending_settings = session.clone();
    let mut snapshot = pending();
    let base = Settings::default();
    assert!(persist_choice(
        &request(Some(5)),
        &base,
        Some(&mut session),
        Some((&mut snapshot, &mut pending_settings)),
        |_, _| Err(AppError::Config("injected persistence failure".into())),
    )
    .is_err());
    for settings in [base, session, pending_settings] {
        assert_eq!(settings.processing_preset, None);
        assert_eq!(settings.processing_target_language, None);
    }
    assert_eq!(snapshot.preset, TextPreset::Clean);
    assert_eq!(snapshot.target_language, None);
}

#[test]
fn choice_survives_restart_and_changes_only_the_two_processing_fields() {
    let base = Settings {
        ai_mode: AiMode::Clean,
        language: "ru".into(),
        ..Settings::default()
    };
    let mut session = base.clone();
    let persisted = persist_choice(
        &request(Some(5)),
        &base,
        Some(&mut session),
        None,
        |_, candidate| Ok(candidate.clone()),
    )
    .unwrap();
    let before = serde_json::to_value(&base).unwrap();
    let after = serde_json::to_value(&persisted).unwrap();
    let changed: Vec<_> = before
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| before.get(*key) != after.get(*key))
        .map(String::as_str)
        .collect();
    assert_eq!(
        changed,
        vec!["processing_preset", "processing_target_language"]
    );
    let restarted: Settings =
        serde_json::from_slice(&serde_json::to_vec(&persisted).unwrap()).unwrap();
    assert_eq!(restarted.processing_preset, session.processing_preset);
    assert_eq!(
        restarted.processing_target_language,
        session.processing_target_language
    );
    assert_eq!(restarted.processing_preset, Some(TextPreset::Formal));
    assert_eq!(restarted.ai_mode, AiMode::Clean);
    assert!(restarted.overlay_quick_processing);
}

#[test]
fn choice_before_resolve_updates_the_session_used_by_the_action() {
    let mut store = Store::default();
    store
        .install(Record {
            snapshot: pending(),
            data: Settings::default(),
        })
        .unwrap();
    let record = editable_pending(&mut store, Some(5), Some(OperationPhase::AwaitingAction))
        .unwrap()
        .unwrap();
    persist_choice(
        &request(Some(5)),
        &Settings::default(),
        None,
        Some((&mut record.snapshot, &mut record.data)),
        |_, settings| Ok(settings.clone()),
    )
    .unwrap();
    let Claim::Work(settings, snapshot) = store
        .claim(&PendingRequest {
            session_id: 5,
            action: PendingAction::ProcessAndInsert,
            preset: None,
            target_language: None,
        })
        .unwrap()
    else {
        panic!("pending owner")
    };
    assert_eq!(snapshot.preset, TextPreset::Formal);
    assert_eq!(snapshot.target_language, Some(TranslationLanguage::En));
    assert_eq!(settings.processing_preset, Some(snapshot.preset));
    assert_eq!(
        settings.processing_target_language,
        snapshot.target_language
    );
}

#[test]
fn resolve_claim_blocks_late_choice_even_before_pipeline_phase_changes() {
    let mut store = Store::default();
    store
        .install(Record {
            snapshot: pending(),
            data: Settings::default(),
        })
        .unwrap();
    store
        .claim(&PendingRequest {
            session_id: 5,
            action: PendingAction::ProcessAndInsert,
            preset: None,
            target_language: None,
        })
        .unwrap();
    assert!(editable_pending(&mut store, Some(5), Some(OperationPhase::AwaitingAction)).is_err());
    assert!(editable_pending(&mut store, None, None).is_err());
    assert!(editable_pending(&mut store, Some(4), Some(OperationPhase::AwaitingAction)).is_err());
}

#[test]
fn null_explicitly_clears_saved_translation_and_request_requires_full_identity() {
    let request: OverlayProcessingChoiceRequest =
        serde_json::from_str(r#"{"sessionId":null,"preset":"task","targetLanguage":null}"#)
            .unwrap();
    let base = Settings {
        processing_target_language: Some(TranslationLanguage::En),
        ..Settings::default()
    };
    let saved = persist_choice(&request, &base, None, None, |_, settings| {
        Ok(settings.clone())
    })
    .unwrap();
    assert_eq!(saved.processing_preset, Some(TextPreset::Task));
    assert_eq!(saved.processing_target_language, None);
    for invalid in [
        r#"{"preset":"task","targetLanguage":null}"#,
        r#"{"sessionId":null,"preset":"task"}"#,
        r#"{"sessionId":1,"preset":"unknown","targetLanguage":null}"#,
        r#"{"sessionId":1,"preset":"task","targetLanguage":"it"}"#,
    ] {
        assert!(serde_json::from_str::<OverlayProcessingChoiceRequest>(invalid).is_err());
    }
    let legacy: Settings = serde_json::from_str("{}").unwrap();
    assert!(legacy.overlay_quick_processing);
}

#[test]
fn response_removes_credentials_from_memory_as_well_as_serialization() {
    let mut settings = Settings {
        llm_api_key: Some("test".into()),
        ..Settings::default()
    };
    settings.llm_profiles[0].api_key = Some("test".into());
    let response = without_secrets(settings);
    assert!(response.llm_api_key.is_none());
    assert!(response
        .llm_profiles
        .iter()
        .all(|profile| profile.api_key.is_none()));
}

#[test]
fn saving_selection_never_enables_processing_or_quick_controls() {
    let base = Settings {
        ai_mode: AiMode::Off,
        overlay_quick_processing: false,
        ..Settings::default()
    };
    let mut session = base.clone();
    let saved = persist_choice(
        &request(Some(5)),
        &base,
        Some(&mut session),
        None,
        |_, settings| Ok(settings.clone()),
    )
    .unwrap();
    for settings in [saved, session] {
        assert_eq!(settings.ai_mode, AiMode::Off);
        assert!(!settings.overlay_quick_processing);
        assert!(!crate::application::dictation::workflow::should_process(
            &settings
        ));
    }
}

#[test]
fn explicit_switches_persist_and_keep_the_translation_language_while_disabled() {
    let base = Settings::default();
    let mut session = base.clone();
    let mut choice = request(Some(5));
    choice.processing_enabled = Some(false);
    choice.translation_enabled = Some(false);
    let saved = persist_choice(&choice, &base, Some(&mut session), None, |_, settings| {
        Ok(settings.clone())
    })
    .unwrap();
    assert_eq!(saved.ai_mode, AiMode::Off);
    assert_eq!(
        saved.processing_target_language,
        Some(TranslationLanguage::En)
    );
    assert!(!saved.processing_translation_enabled);
    assert_eq!(super::super::types::effective_language(&saved), None);
    choice.processing_enabled = Some(true);
    let restored = persist_choice(&choice, &saved, Some(&mut session), None, |_, settings| {
        Ok(settings.clone())
    })
    .unwrap();
    assert_eq!(restored.ai_mode, AiMode::Clean);
    assert_eq!(
        restored.processing_target_language,
        Some(TranslationLanguage::En)
    );
    assert_eq!(super::super::types::effective_language(&restored), None);
}

#[test]
fn pending_uses_the_effective_language_without_losing_the_saved_choice() {
    let base = Settings::default();
    let mut settings = base.clone();
    let mut snapshot = pending();
    let mut choice = request(Some(5));
    choice.translation_enabled = Some(false);
    persist_choice(
        &choice,
        &base,
        None,
        Some((&mut snapshot, &mut settings)),
        |_, candidate| Ok(candidate.clone()),
    )
    .unwrap();
    assert_eq!(
        settings.processing_target_language,
        Some(TranslationLanguage::En)
    );
    assert_eq!(snapshot.target_language, None);
    assert!(!snapshot.translation_enabled);
    choice.translation_enabled = Some(true);
    persist_choice(
        &choice,
        &base,
        None,
        Some((&mut snapshot, &mut settings)),
        |_, candidate| Ok(candidate.clone()),
    )
    .unwrap();
    assert_eq!(snapshot.target_language, Some(TranslationLanguage::En));
    assert!(snapshot.translation_enabled);
}
