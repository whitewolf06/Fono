use super::*;
use crate::types::{DictationHistoryMetadata, DictationTimingMeasurements, SpeechTimingSummary};
fn settings() -> Settings {
    Settings {
        analytics_enabled: true,
        analytics_retention_days: 90,
        ..Settings::default()
    }
}
fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-21T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn timing(speech: u64, pauses: u64, count: u32, max: u64) -> SpeechTimingSummary {
    SpeechTimingSummary {
        schema_version: 1,
        speech_time_ms: speech,
        pause_count: count,
        pause_time_ms: pauses,
        max_pause_ms: max,
    }
}
fn entry(
    id: usize,
    time: DateTime<Utc>,
    value: Option<SpeechTimingSummary>,
) -> DictationHistoryEntry {
    let mut entry: DictationHistoryEntry = serde_json::from_value(serde_json::json!({
        "id":id.to_string(),"text":"separate result", "created_at":time,"original_text":"один два три",
        "analysis_status":"ready", "analysis":crate::speech_metrics::analyze_russian_speech("один два три")
    })).unwrap();
    entry.metadata = Some(DictationHistoryMetadata::from_result(
        &settings(),
        &serde_json::from_value(serde_json::json!({"text":"один два три"})).unwrap(),
        DictationTimingMeasurements {
            speech_timing: value,
            recording_duration_ms: Some(10000),
            ..Default::default()
        },
    ));
    entry
}
#[test]
fn full_30_and_90_day_periods_use_only_matched_words_and_speech_denominators() {
    for days in [30, 90] {
        let records = (0..540)
            .map(|index| {
                entry(
                    index,
                    now() - Duration::hours((index as i64) % (i64::from(days) * 24 - 12)),
                    (index % 2 == 0).then_some(timing(2000, 1000, 2, 750)),
                )
            })
            .collect::<Vec<_>>();
        let snapshot = build_snapshot(
            &records,
            Period::new(days, now()).unwrap(),
            &settings(),
            now(),
            "test".into(),
        );
        assert_eq!(snapshot.included_sessions, 540);
        let t = snapshot.speech_timing.unwrap();
        assert_eq!(
            (
                t.measured_sessions,
                t.unknown_sessions,
                t.words,
                t.speech_time_ms,
                t.pause_count,
                t.max_pause_ms
            ),
            (270, 270, 810, 540000, 540, 750)
        );
        assert_eq!(t.words * 60000 / t.speech_time_ms, 90);
        assert_eq!(
            snapshot
                .daily
                .iter()
                .map(|day| day.speech_timing.as_ref().unwrap().measured_sessions)
                .sum::<u32>(),
            270
        );
    }
}
#[test]
fn zero_pauses_are_known_but_legacy_and_malformed_summaries_are_unknown() {
    let mut records = vec![
        entry(0, now(), Some(timing(2000, 0, 0, 0))),
        entry(1, now(), None),
    ];
    records.push(entry(2, now(), None));
    records[2].metadata.as_mut().unwrap().speech_timing = Some(timing(0, 2000, 1, 2000));
    let snapshot = build_snapshot(
        &records,
        Period::new(30, now()).unwrap(),
        &settings(),
        now(),
        "test".into(),
    );
    let t = snapshot.speech_timing.unwrap();
    assert_eq!(
        (
            t.measured_sessions,
            t.unknown_sessions,
            t.words,
            t.pause_count
        ),
        (1, 2, 3, 0)
    );
}
#[test]
fn consent_erases_summary_and_all_cached_periods_without_erasing_technical_facts() {
    let mut entry = entry(0, now(), Some(timing(2000, 1000, 1, 1000)));
    let mut cache = TrainerCache::default();
    get_from_cache(&mut cache, &[entry.clone()], 30, true, &settings(), now()).unwrap();
    let before = source_fingerprint(&entry).unwrap();
    entry.metadata.as_mut().unwrap().speech_timing = Some(timing(3000, 1000, 1, 1000));
    assert_ne!(before, source_fingerprint(&entry).unwrap());
    entry.clear_analytics_data(DictationAnalysisStatus::Disabled);
    assert!(entry.metadata.as_ref().unwrap().speech_timing.is_none());
    assert_eq!(entry.metadata.unwrap().recording_duration_ms, Some(10000));
    sanitize_cache(&mut cache);
    assert!(cache.periods[0].snapshot.speech_timing.is_none());
    assert!(cache.periods[0]
        .snapshot
        .daily
        .iter()
        .all(|day| day.speech_timing.is_none()));
    let metadata = DictationHistoryMetadata::from_result(
        &Settings::default(),
        &serde_json::from_value(serde_json::json!({"text":"x"})).unwrap(),
        DictationTimingMeasurements {
            speech_timing: Some(timing(2000, 0, 0, 0)),
            ..Default::default()
        },
    );
    assert!(metadata.speech_timing.is_none());
}
