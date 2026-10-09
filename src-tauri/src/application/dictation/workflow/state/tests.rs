use super::*;
use crate::application::dictation::workflow::{
    types::PendingAction, TextPreset, TranslationLanguage,
};
fn record(id: u64) -> Record<()> {
    Record {
        data: (),
        snapshot: PendingDictation {
            session_id: id,
            phase: PendingPhase::AwaitingAction,
            original_text: "raw".into(),
            result_text: None,
            created_at: "date".into(),
            preset: TextPreset::Clean,
            target_language: None,
            processing_enabled: true,
            translation_enabled: true,
            source: fono_core::OperationSource::Ui,
            error: None,
            insertion_blocked: false,
            copy_only: false,
        },
    }
}
fn request(id: u64) -> PendingRequest {
    PendingRequest {
        session_id: id,
        action: PendingAction::ProcessAndInsert,
        preset: None,
        target_language: None,
    }
}
#[test]
fn double_actions_and_new_capture_cannot_replace_pending_owner() {
    let mut store = Store::default();
    store.install(record(1)).unwrap();
    assert!(store.install(record(2)).is_err());
    assert!(matches!(store.claim(&request(1)).unwrap(), Claim::Work(..)));
    assert!(store.claim(&request(1)).is_err());
}
#[test]
fn processing_error_preserves_raw_text_and_allows_retry_with_session_overrides() {
    let mut store = Store::default();
    store.install(record(1)).unwrap();
    store.claim(&request(1)).unwrap();
    assert!(store.retry(1, "unavailable".into(), false));
    let mut retry = request(1);
    retry.preset = Some(TextPreset::Task);
    retry.target_language = Some(Some(TranslationLanguage::En));
    let Claim::Work(_, snapshot) = store.claim(&retry).unwrap() else {
        panic!()
    };
    assert_eq!(snapshot.original_text, "raw");
    assert_eq!(snapshot.preset, TextPreset::Task);
    assert_eq!(snapshot.target_language, Some(TranslationLanguage::En));
    assert!(snapshot.translation_enabled);
}
#[test]
fn cancel_and_late_completion_cannot_touch_a_new_session() {
    let mut store = Store::default();
    store.install(record(1)).unwrap();
    store.claim(&request(1)).unwrap();
    assert!(store.finish(1, None));
    store.install(record(2)).unwrap();
    assert!(!store.finish(1, None));
    assert!(!store.retry(1, "late".into(), false));
    assert!(store.claim(&request(1)).is_err());
    assert_eq!(store.snapshot().unwrap().session_id, 2);
}
#[test]
fn terminal_action_is_idempotent_and_blocked_insertion_cannot_be_repeated() {
    let mut store = Store::default();
    store.install(record(1)).unwrap();
    assert!(store.finish(1, None));
    assert!(matches!(
        store.claim(&request(1)).unwrap(),
        Claim::Completed(None)
    ));
    store.install(record(2)).unwrap();
    store.retry(2, "partial".into(), true);
    assert!(store.claim(&request(2)).is_err());
}
#[test]
fn processed_result_is_kept_for_copy_after_blocked_insertion() {
    let mut store = Store::default();
    store.install(record(1)).unwrap();
    store.claim(&request(1)).unwrap();
    assert!(store.generated(1, "processed".into()));
    assert!(store.retry(1, "field changed".into(), true));
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.result_text.as_deref(), Some("processed"));
    assert_eq!(snapshot.original_text, "raw");
    let json = serde_json::to_value(snapshot).unwrap();
    assert_eq!(json["resultText"], "processed");
    assert!(json.get("settings").is_none());
}
#[test]
fn only_first_success_can_archive_and_stale_output_cannot_replace_new_pending() {
    let mut store = Store::default();
    store.install(record(1)).unwrap();
    let transcript = Transcript {
        text: "final".into(),
        detected_language: None,
        transcribe_secs: None,
        audio_secs: None,
        device: None,
    };
    assert!(store.finish(1, Some(transcript.clone())));
    assert!(!store.finish(1, Some(transcript)));
    let Claim::Completed(Some(output)) = store.claim(&request(1)).unwrap() else {
        panic!()
    };
    assert_eq!(output.text, "final");
    store.install(record(2)).unwrap();
    assert!(!store.generated(1, "late".into()));
    assert!(store.snapshot().unwrap().result_text.is_none());
}
#[test]
fn session_overrides_never_change_the_stored_default_settings() {
    let mut store = Store::default();
    let original = record(1);
    store
        .install(Record {
            snapshot: original.snapshot,
            data: crate::types::Settings::default(),
        })
        .unwrap();
    let mut action = request(1);
    action.preset = Some(TextPreset::Formal);
    action.target_language = Some(Some(TranslationLanguage::De));
    let Claim::Work(settings, snapshot) = store.claim(&action).unwrap() else {
        panic!()
    };
    assert_eq!(snapshot.preset, TextPreset::Formal);
    assert_eq!(settings.processing_preset, None);
    assert_eq!(settings.processing_target_language, None);
    assert_eq!(store.pending.unwrap().data.processing_preset, None);
}
#[test]
fn detached_pending_data_and_active_work_share_lease_until_actual_work_drops() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct LeaseProbe(Arc<AtomicBool>);
    impl Drop for LeaseProbe {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let active = Arc::new(AtomicBool::new(true));
    let lease = Arc::new(LeaseProbe(active.clone()));
    let mut store = Store::default();
    let snapshot = record(1).snapshot;
    store
        .install(Record {
            snapshot,
            data: lease,
        })
        .unwrap();
    let Claim::Work(work, _) = store.claim(&request(1)).unwrap() else {
        panic!()
    };
    let detached = store.detach(1, None).unwrap();
    assert!(active.load(Ordering::SeqCst));
    drop(detached);
    assert!(active.load(Ordering::SeqCst));
    drop(work);
    assert!(!active.load(Ordering::SeqCst));
}

#[test]
fn blocked_insertion_still_allows_copy_completion_but_never_another_insert() {
    let mut store = Store::default();
    store.install(record(1)).unwrap();
    store.generated(1, "finished".into());
    store.retry(1, "field changed".into(), true);
    assert!(store.claim(&request(1)).is_err());
    let mut complete = request(1);
    complete.action = PendingAction::Complete;
    let Claim::Work(_, snapshot) = store.claim(&complete).unwrap() else {
        panic!()
    };
    assert_eq!(snapshot.result_text.as_deref(), Some("finished"));
    assert!(store.finish(1, None));
    assert!(!store.finish(1, None));
}

#[test]
fn copy_only_rejects_both_insertion_actions_without_claiming_the_result() {
    let mut store = Store::default();
    let mut preview = record(1);
    preview.snapshot.copy_only = true;
    store.install(preview).unwrap();
    for action in [PendingAction::InsertRaw, PendingAction::ProcessAndInsert] {
        let mut insertion = request(1);
        insertion.action = action;
        assert!(store.claim(&insertion).is_err());
        assert_eq!(
            store.snapshot().unwrap().phase,
            PendingPhase::AwaitingAction
        );
    }
    let mut processing = request(1);
    processing.action = PendingAction::ProcessPreview;
    assert!(matches!(store.claim(&processing).unwrap(), Claim::Work(..)));
    store.retry(1, "retry".into(), false);
    processing.action = PendingAction::Complete;
    assert!(matches!(store.claim(&processing).unwrap(), Claim::Work(..)));
}

#[test]
fn explicit_translation_override_changes_the_effective_pending_choice() {
    let mut store = Store::default();
    let mut preview = record(1);
    preview.snapshot.translation_enabled = false;
    store.install(preview).unwrap();
    let mut choice = request(1);
    choice.target_language = Some(Some(TranslationLanguage::En));
    let Claim::Work(_, enabled) = store.claim(&choice).unwrap() else {
        panic!()
    };
    assert!(enabled.translation_enabled);
    assert_eq!(enabled.target_language, Some(TranslationLanguage::En));
    store.retry(1, "retry".into(), false);
    choice.target_language = Some(None);
    let Claim::Work(_, disabled) = store.claim(&choice).unwrap() else {
        panic!()
    };
    assert!(!disabled.translation_enabled);
    assert_eq!(disabled.target_language, None);
}
