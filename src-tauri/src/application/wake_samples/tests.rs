use super::*;

struct Temporary(std::path::PathBuf);
impl Temporary {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("fono-wake-samples-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        // Fixture is a fresh, unique temp directory; it never contains user recordings.
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request() -> SampleRequest {
    SampleRequest {
        language: Language::Ru,
        kind: SampleKind::Phrase,
        condition: Condition::Quiet,
        consent: true,
    }
}
#[test]
fn recording_keeps_exact_pcm_and_private_provenance_without_invented_annotation() {
    let root = Temporary::new();
    let samples: Vec<i16> = (0..RATE).map(|i| (i % 32_000) as i16 - 16_000).collect();
    let sample = save_at(&root.0, request(), &samples, "session-one").unwrap();
    let dir = entry_dir(&root.0, &sample.id).unwrap();
    let bytes = fs::read(dir.join("recording.wav")).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        16_000
    );
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16);
    assert_eq!(
        &bytes[44..],
        samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(sample.duration_ms, 1000);
    assert_eq!(sample.sha256, format!("{:x}", Sha256::digest(&bytes)));
    let meta: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("recording.json")).unwrap()).unwrap();
    assert_eq!(meta["split"], "dev");
    assert_eq!(meta["permission"], "consented-local-only");
    assert!(meta["annotation"].is_null());
    assert_eq!(meta["speaker"], meta["sourceGroup"]);
    assert!(Uuid::parse_str(meta["speaker"].as_str().unwrap()).is_ok());
    assert_eq!(load(&root.0).unwrap().len(), 1);
}
#[test]
fn recordings_across_sessions_keep_the_same_speaker_and_development_split() {
    let root = Temporary::new();
    let first = save_at(&root.0, request(), &vec![5; RATE], "first").unwrap();
    let mut second_request = request();
    second_request.language = Language::En;
    let second = save_at(&root.0, second_request, &vec![6; RATE], "second").unwrap();
    let read = |sample: Sample| -> Recording {
        serde_json::from_slice(&fs::read(root.0.join(sample.id).join("recording.json")).unwrap())
            .unwrap()
    };
    let (a, b) = (read(first), read(second));
    assert_eq!(a.speaker, b.speaker);
    assert_eq!(a.source_group, b.source_group);
    assert_eq!(a.split, b.split);
    assert_ne!(a.session, b.session);
}
#[test]
fn rejected_consent_does_not_create_voice_files() {
    let root = Temporary::new();
    let mut request = request();
    request.consent = false;
    assert!(save_at(&root.0, request, &vec![2; RATE], "session").is_err());
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
}
#[test]
fn recording_limits_reject_empty_short_and_overlong_audio() {
    assert!(wav(&[]).is_err());
    assert!(wav(&vec![1; RATE / 4 - 1]).is_err());
    assert!(wav(&vec![1; RATE * 22 + 1]).is_err());
    let root = Temporary::new();
    assert!(save_at(&root.0, request(), &vec![1; RATE * 8], "session").is_err());
}
#[test]
fn incomplete_transaction_is_hidden_and_deletion_removes_audio_and_metadata() {
    let root = Temporary::new();
    let pending = root.0.join(format!(".{}.pending", Uuid::new_v4()));
    fs::create_dir(pending).unwrap();
    let sample = save_at(&root.0, request(), &vec![1; RATE], "session").unwrap();
    assert_eq!(load(&root.0).unwrap().len(), 1);
    delete_at(&root.0, &sample.id).unwrap();
    assert!(load(&root.0).unwrap().is_empty());
    assert!(!root.0.join(&sample.id).exists());
    assert!(!root.0.join(format!(".{}.deleted", sample.id)).exists());
}
#[test]
fn identifiers_cannot_address_files_outside_the_recording_directory() {
    let root = Temporary::new();
    for id in [
        "../settings.json",
        "C:/anything",
        "",
        "00000000-0000-0000-0000-000000000000/..",
    ] {
        assert!(entry_dir(&root.0, id).is_err());
        assert!(delete_at(&root.0, id).is_err());
    }
}

#[test]
fn playback_verifies_the_recording_hash_and_deletion_can_retry_partial_cleanup() {
    let root = Temporary::new();
    let sample = save_at(&root.0, request(), &vec![1; RATE], "session").unwrap();
    let audio = STANDARD
        .decode(read_audio_at(&root.0, &sample.id).unwrap())
        .unwrap();
    assert_eq!(audio.len(), RATE * 2 + 44);
    fs::write(root.0.join(&sample.id).join("recording.wav"), vec![0; 44]).unwrap();
    assert!(read_audio_at(&root.0, &sample.id).is_err());
    let tombstone = root.0.join(format!(".{}.deleted", sample.id));
    fs::rename(root.0.join(&sample.id), &tombstone).unwrap();
    fs::remove_file(tombstone.join("recording.wav")).unwrap();
    assert!(load(&root.0).unwrap().is_empty());
    delete_at(&root.0, &sample.id).unwrap();
    assert!(!tombstone.exists());
}

#[test]
fn cancel_during_file_preparation_removes_the_pending_voice_before_publication() {
    let root = Temporary::new();
    let checks = std::cell::Cell::new(0);
    let result = save_active_at(&root.0, request(), &vec![2; RATE], "session", || {
        checks.set(checks.get() + 1);
        checks.get() == 1
    });
    assert!(matches!(result, Err(AppError::Cancelled(_))));
    assert!(load(&root.0).unwrap().is_empty());
    // Only the opaque speaker identity remains; WAV/JSON transaction data is gone.
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
}
