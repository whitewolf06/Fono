use super::*;
use crate::types::{
    ApplicationIdentity, DictationHistoryMetadata, InjectionMode, InsertionObservation, Transcript,
};
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("fono-app-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn repo(&self) -> HistoryRepository {
        HistoryRepository::at_test_directory(self.0.clone())
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn settings() -> Settings {
    Settings {
        application_context_enabled: true,
        history_enabled: true,
        ..Settings::default()
    }
}
fn entry(id: &str) -> DictationHistoryEntry {
    let transcript = Transcript {
        text: "Preserved text".into(),
        detected_language: None,
        transcribe_secs: None,
        audio_secs: Some(2.0),
        device: Some("CPU".into()),
    };
    let mut e:DictationHistoryEntry=serde_json::from_value(serde_json::json!({"id":id,"text":"Preserved text","created_at":chrono::Utc::now(),"device":"CPU"})).unwrap();
    e.metadata = Some(DictationHistoryMetadata::from_result(
        &settings(),
        &transcript,
        Default::default(),
    ));
    e
}
fn context() -> ApplicationContext {
    ApplicationContext {
        before: ApplicationIdentity::from_executable("Code.exe"),
        after: ApplicationIdentity::from_executable("Code.exe"),
        outcome: InsertionObservation::Sent,
        method: Some(InjectionMode::Clipboard),
        attempted: true,
    }
}
fn append(repo: &HistoryRepository, id: &str, permit: Option<u64>) {
    repo.append_with_application_context(
        entry(id),
        true,
        permit.map(|p| (p, context())),
        false,
        30,
    )
    .unwrap();
}
#[test]
fn legacy_settings_and_history_do_not_invent_context_or_consent() {
    let s: Settings = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(!s.application_context_enabled);
    let d = Directory::new();
    let r = d.repo();
    r.maintain(&s).unwrap();
    assert!(r.application_context_permit().is_none());
    append(&r, "old", None);
    assert!(r.list(false, 30).unwrap()[0]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_none());
}
#[test]
fn context_persists_without_speech_analytics_and_reopens_unchanged() {
    let d = Directory::new();
    {
        let r = d.repo();
        r.maintain(&settings()).unwrap();
        append(&r, "entry", r.application_context_permit());
    }
    let r = d.repo();
    r.maintain(&settings()).unwrap();
    let e = &r.list(false, 30).unwrap()[0];
    assert_eq!(e.text, "Preserved text");
    assert!(e.original_text.is_none());
    let json = serde_json::to_value(
        e.metadata
            .as_ref()
            .unwrap()
            .application_context
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["before"]["id"], "exe:code.exe");
    assert_eq!(json["outcome"], "sent");
    for forbidden in [
        "window",
        "focus",
        "process",
        "runtime_id",
        "path",
        "title",
        "url",
        "delivery_confirmed",
    ] {
        assert!(json.get(forbidden).is_none());
    }
}
#[test]
fn disable_and_reenable_cannot_restore_context_from_a_queued_old_session() {
    let d = Directory::new();
    let r = d.repo();
    let mut s = settings();
    r.maintain(&s).unwrap();
    let old = r.application_context_permit();
    append(&r, "kept", old);
    s.application_context_enabled = false;
    r.maintain(&s).unwrap();
    s.application_context_enabled = true;
    r.maintain(&s).unwrap();
    append(&r, "late", old);
    let entries = r.list(false, 30).unwrap();
    assert!(entries[0]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_none());
    assert!(entries[1]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_some());
    append(&r, "fresh", r.application_context_permit());
    assert!(r.list(false, 30).unwrap()[0]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_some());
}
#[test]
fn explicit_context_erasure_preserves_text_and_invalidates_pending_context() {
    let d = Directory::new();
    let r = d.repo();
    r.maintain(&settings()).unwrap();
    let old = r.application_context_permit();
    append(&r, "one", old);
    append(&r, "two", old);
    r.clear_application_context(Some("one")).unwrap();
    let e = r.list(false, 30).unwrap();
    assert!(e.iter().all(|e| e.text == "Preserved text"));
    assert!(e[0]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_some());
    assert!(e[1]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_none());
    append(&r, "late", old);
    assert!(r.list(false, 30).unwrap()[0]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_none());
    r.clear_application_context(None).unwrap();
    assert!(r.list(false, 30).unwrap().iter().all(|e| e
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_none()));
}
#[test]
fn history_deletion_and_clear_leave_no_context_or_inflight_resurrection() {
    let d = Directory::new();
    let r = d.repo();
    r.maintain(&settings()).unwrap();
    let old = r.application_context_permit();
    append(&r, "one", old);
    r.delete("one").unwrap();
    assert!(r.list(false, 30).unwrap().is_empty());
    append(&r, "two", r.application_context_permit());
    r.clear().unwrap();
    append(&r, "late", old);
    assert!(r.list(false, 30).unwrap()[0]
        .metadata
        .as_ref()
        .unwrap()
        .application_context
        .is_none());
}
