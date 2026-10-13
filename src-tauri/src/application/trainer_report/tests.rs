use super::*;
use crate::types::SpeechLlmDataScope;

fn date(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn settings() -> Settings {
    Settings {
        history_enabled: true,
        analytics_enabled: true,
        speech_trainer_enabled: true,
        analytics_retention_days: 90,
        ..Settings::default()
    }
}

fn entry(id: &str, created_at: DateTime<Utc>, text: &str) -> DictationHistoryEntry {
    DictationHistoryEntry {
        id: id.into(),
        text: "separate final text".into(),
        created_at,
        device: None,
        metadata: None,
        analytics_included: true,
        original_text: Some(text.into()),
        processing: None,
        analysis_status: DictationAnalysisStatus::Ready,
        analysis: Some(crate::speech_metrics::analyze_russian_speech(text)),
        analysis_error: None,
        recommendation_status: DictationAnalysisStatus::Pending,
        recommendation: None,
        recommendation_error: None,
    }
}

fn context() -> TrainerAiContext {
    TrainerAiContext {
        data_scope: SpeechLlmDataScope::Findings,
        payload_bytes: 4321,
        payload_byte_limit: 48000,
        reduced: true,
        metrics_sessions: 13,
        metrics_words: 500,
        available_sessions: 13,
        shared_sessions: 0,
        omitted_sessions: 13,
        shortened_sessions: 0,
        available_words: 500,
        shared_words: 0,
        omitted_words: 500,
        available_characters: 2500,
        shared_characters: 0,
        omitted_characters: 2500,
        patterns_available: 43,
        patterns_shared: 40,
        examples_available: 120,
        examples_shared: 80,
        limits: TrainerAiContextLimits {
            max_shared_sessions: 12,
            max_characters_per_session: 3000,
            max_patterns: 40,
            max_examples_per_pattern: 2,
        },
    }
}

fn summary(snapshot_fp: &str) -> TrainerAiSummary {
    TrainerAiSummary {
        fingerprint: "ai-parameters".into(),
        snapshot_fingerprint: snapshot_fp.into(),
        generated_at: "2026-10-12T12:01:00Z".into(),
        profile_id: "fixture".into(),
        profile_name: "Fixture local".into(),
        model: "fixture-model".into(),
        data_scope: SpeechLlmDataScope::Findings,
        context: Some(context()),
        summary: "privatefixture coaching".into(),
        priorities: vec![TrainerAiPriority {
            title: "priority".into(),
            basis: "local pattern counts".into(),
            exercise: "exercise".into(),
        }],
    }
}

#[test]
fn calendar_range_is_fixed_within_utc_day_and_exclusive_at_next_midnight() {
    let now = date("2026-10-12T12:00:00Z");
    let period = Period::new(7, now).unwrap();
    assert_eq!(timestamp(period.from), "2026-10-06T00:00:00Z");
    assert_eq!(timestamp(period.to), "2026-10-13T00:00:00Z");
    let entries = vec![
        entry("before", period.from - Duration::nanoseconds(1), "ну"),
        entry("first", period.from, "ну"),
        entry("last", period.to - Duration::nanoseconds(1), "ну"),
        entry("after", period.to, "ну"),
    ];
    let eligible = eligible_sources(&entries, period, &settings(), now);
    assert_eq!(
        eligible
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "last"]
    );
    let mut cache = TrainerCache::default();
    let (report, _) = get_from_cache(&mut cache, &entries, 7, false, &settings(), now).unwrap();
    assert_eq!(report.snapshot.daily.len(), 7);
    assert_eq!(report.snapshot.daily[0].sessions, 1);
    assert_eq!(report.snapshot.daily[1].sessions, 0);
    assert_eq!(report.snapshot.daily[6].sessions, 1);
    assert!(Period::new(14, now).is_err());
}

#[test]
fn fingerprint_ignores_clock_ticks_record_order_and_ai_output_but_tracks_sources_and_policy() {
    let now = date("2026-10-12T12:00:00Z");
    let period = Period::new(7, now).unwrap();
    let mut records = vec![entry("a", now, "Ну я я"), entry("b", now, "то есть")];
    let config = settings();
    let original = fingerprint_for(&records, period, &config, now, 0).unwrap();
    records.reverse();
    records[0].recommendation_status = DictationAnalysisStatus::Ready;
    assert_eq!(
        original,
        fingerprint_for(
            &records,
            Period::new(7, now + Duration::hours(1)).unwrap(),
            &config,
            now + Duration::hours(1),
            0
        )
        .unwrap()
    );
    assert_ne!(
        original,
        fingerprint_for(&records, period, &config, now, 1).unwrap()
    );
    records[0].analytics_included = false;
    assert_ne!(
        original,
        fingerprint_for(&records, period, &config, now, 0).unwrap()
    );
    records[0].analytics_included = true;
    records[0].original_text = Some("new original".into());
    assert_ne!(
        original,
        fingerprint_for(&records, period, &config, now, 0).unwrap()
    );
    let mut changed_policy = config.clone();
    changed_policy.analytics_retention_days = 30;
    assert_ne!(
        original,
        fingerprint_for(&records, period, &changed_policy, now, 0).unwrap()
    );
    assert_ne!(
        original,
        fingerprint_for(
            &records,
            Period::new(7, now + Duration::days(1)).unwrap(),
            &config,
            now + Duration::days(1),
            0
        )
        .unwrap()
    );
}

#[test]
fn only_completed_included_unexpired_opt_in_sources_contribute() {
    let now = date("2026-10-12T12:00:00Z");
    let config = settings();
    let ready = entry("ready", now, "ну я я");
    let mut excluded = ready.clone();
    excluded.id = "excluded".into();
    excluded.analytics_included = false;
    let mut pending = ready.clone();
    pending.id = "pending".into();
    pending.analysis_status = DictationAnalysisStatus::Pending;
    let mut failed = ready.clone();
    failed.id = "failed".into();
    failed.analysis_status = DictationAnalysisStatus::Failed;
    let mut no_original = ready.clone();
    no_original.id = "no-original".into();
    no_original.original_text = None;
    let mut no_analysis = ready.clone();
    no_analysis.id = "no-analysis".into();
    no_analysis.analysis = None;
    let expired = entry("expired", now - Duration::days(90), "ну");
    assert!(!eligible(&expired, &config, now));
    let records = vec![ready, excluded, pending, failed, no_original, no_analysis];
    let mut cache = TrainerCache::default();
    let (report, _) = get_from_cache(&mut cache, &records, 7, false, &config, now).unwrap();
    assert_eq!(report.snapshot.included_sessions, 1);
    assert_eq!(report.snapshot.excluded_sessions, 1);
    assert_eq!(report.snapshot.pending_sessions, 1);
    assert_eq!(report.snapshot.unavailable_sessions, 3);
    assert_eq!(report.snapshot.total_words, 3);
    assert_eq!(report.snapshot.session_ids, vec!["ready"]);
    for turn_off in 0..3 {
        let mut disabled = config.clone();
        match turn_off {
            0 => disabled.analytics_enabled = false,
            1 => disabled.history_enabled = false,
            _ => disabled.speech_trainer_enabled = false,
        }
        assert!(
            eligible_sources(&records, Period::new(7, now).unwrap(), &disabled, now).is_empty()
        );
    }
}

#[test]
fn report_sources_match_fresh_eligibility_and_are_hidden_until_refresh_after_source_loss() {
    let now = date("2026-10-12T12:00:00Z");
    let mut config = settings();
    config.analytics_retention_days = 1;
    let older = entry("older", date("2026-10-11T12:05:00Z"), "ну я я");
    let younger = entry("younger", now - Duration::hours(2), "то есть слова");
    let mut pending = entry("pending", now, "не готовые слова");
    pending.analysis_status = DictationAnalysisStatus::Pending;
    let outside = entry("outside", now - Duration::days(10), "вне периода");
    let records = vec![younger, pending, older, outside];
    let mut cache = TrainerCache::default();
    let (initial, sources, written) =
        get_from_cache_with_sources(&mut cache, &records, 7, false, &config, now).unwrap();
    assert!(written);
    assert!(!initial.stale);
    assert_eq!(initial.snapshot.included_sessions, 2);
    assert_eq!(initial.snapshot.total_words, 6);
    assert_eq!(
        sources
            .iter()
            .map(|source| source.id.as_str())
            .collect::<Vec<_>>(),
        vec!["older", "younger"]
    );
    assert_eq!(
        sources.iter().map(|source| &source.id).collect::<Vec<_>>(),
        initial.snapshot.session_ids.iter().collect::<Vec<_>>()
    );
    for mutation in 0..3 {
        let mut current_records = records.clone();
        let checked_at = match mutation {
            0 => {
                current_records
                    .iter_mut()
                    .find(|source| source.id == "older")
                    .unwrap()
                    .analytics_included = false;
                now
            }
            1 => {
                current_records.retain(|source| source.id != "older");
                now
            }
            _ => now + Duration::minutes(10),
        };
        let mut current_cache = cache.clone();
        let (stale, stale_sources, written) = get_from_cache_with_sources(
            &mut current_cache,
            &current_records,
            7,
            false,
            &config,
            checked_at,
        )
        .unwrap();
        assert!(!written);
        assert!(stale.stale);
        assert!(stale_sources.is_empty());
        assert_eq!(stale.snapshot.counts, initial.snapshot.counts);
        assert_eq!(stale.snapshot.generated_at, initial.snapshot.generated_at);
        let (refreshed, refreshed_sources, written) = get_from_cache_with_sources(
            &mut current_cache,
            &current_records,
            7,
            true,
            &config,
            checked_at,
        )
        .unwrap();
        assert!(written);
        assert!(!refreshed.stale);
        assert_eq!(refreshed.snapshot.included_sessions, 1);
        assert_eq!(refreshed.snapshot.total_words, 3);
        assert_eq!(
            refreshed_sources
                .iter()
                .map(|source| source.id.as_str())
                .collect::<Vec<_>>(),
            vec!["younger"]
        );
        assert_eq!(refreshed.snapshot.session_ids, vec!["younger"]);
    }
}

#[test]
fn patterns_are_ranked_normalized_explainable_and_use_all_marker_density() {
    let now = date("2026-10-12T12:00:00Z");
    let records = (0..5)
        .map(|index| {
            entry(
                &index.to_string(),
                now,
                "Ну, КАК бы я я, то есть, точнее объяснить это...",
            )
        })
        .collect::<Vec<_>>();
    let mut cache = TrainerCache::default();
    let (report, _) = get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    let snapshot = report.snapshot;
    assert_eq!(
        snapshot.counts,
        TrainerCounts {
            filler: 10,
            repetition: 5,
            self_correction: 10,
            unfinished: 5
        }
    );
    assert_eq!(snapshot.total_words, 50);
    assert_eq!(snapshot.density, 60.0);
    assert_eq!(snapshot.daily.last().unwrap().density, 60.0);
    let parasite = snapshot
        .terms
        .iter()
        .find(|term| term.key == "filler:как бы")
        .unwrap();
    assert_eq!(parasite.label, "как бы");
    assert_eq!(parasite.count, 5);
    assert_eq!(parasite.affected_sessions, 5);
    assert_eq!(parasite.examples.len(), MAX_EXAMPLES);
    assert_eq!(parasite.density, 10.0);
    assert!(snapshot.terms.iter().any(|term| term.key == "repetition:я"));
    assert!(snapshot
        .terms
        .iter()
        .any(|term| term.key == "unfinished:unfinished"));
    assert_eq!(
        normalized_term(SpeechFindingKind::Repetition, "Ёлка ЕЛКА").0,
        "repetition:елка"
    );
}

#[test]
fn cached_report_stays_frozen_until_explicit_refresh_and_hides_stale_private_payloads() {
    let now = date("2026-10-12T12:00:00Z");
    let mut records = vec![entry("a", now, "privatefixture privatefixture ну")];
    let mut cache = TrainerCache::default();
    let (first, written) =
        get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    assert!(written);
    cache.periods[0].ai_summary = Some(summary(&first.current_fingerprint));
    let (reopened, written) = get_from_cache(
        &mut cache,
        &records,
        7,
        false,
        &settings(),
        now + Duration::minutes(1),
    )
    .unwrap();
    assert!(!written);
    assert_eq!(reopened.snapshot.generated_at, first.snapshot.generated_at);
    assert!(reopened.ai_summary.is_some());
    records.push(entry("b", now, "additional additional words"));
    let (stale, written) = get_from_cache(
        &mut cache,
        &records,
        7,
        false,
        &settings(),
        now + Duration::minutes(2),
    )
    .unwrap();
    assert!(!written);
    assert!(stale.stale);
    assert_eq!(stale.snapshot.fingerprint, first.snapshot.fingerprint);
    assert_eq!(stale.snapshot.counts, first.snapshot.counts);
    assert_eq!(stale.snapshot.generated_at, first.snapshot.generated_at);
    assert!(stale.snapshot.terms.is_empty());
    assert!(stale.snapshot.session_ids.is_empty());
    assert!(stale.ai_summary.is_none());
    let (refreshed, written) = get_from_cache(
        &mut cache,
        &records,
        7,
        true,
        &settings(),
        now + Duration::minutes(3),
    )
    .unwrap();
    assert!(written);
    assert!(!refreshed.stale);
    assert_eq!(refreshed.snapshot.included_sessions, 2);
    assert_ne!(refreshed.snapshot.fingerprint, first.snapshot.fingerprint);
    assert!(refreshed.ai_summary.is_none());
}

#[test]
fn privacy_sanitization_keeps_old_numbers_but_purges_all_cached_text_and_source_ids() {
    let now = date("2026-10-12T12:00:00Z");
    let records = vec![entry(
        "private-session",
        now,
        "privatefixture privatefixture ну",
    )];
    let mut cache = TrainerCache::default();
    for days in [7, 30, 90] {
        let (report, _) =
            get_from_cache(&mut cache, &records, days, false, &settings(), now).unwrap();
        cache
            .periods
            .iter_mut()
            .find(|saved| saved.snapshot.days == days)
            .unwrap()
            .ai_summary = Some(summary(&report.current_fingerprint));
    }
    let before = cache.periods[0].snapshot.clone();
    sanitize_cache(&mut cache);
    let serialized = serde_json::to_string(&cache).unwrap();
    assert!(!serialized.contains("privatefixture"));
    assert!(!serialized.contains("private-session"));
    assert_eq!(cache.periods[0].snapshot.counts, before.counts);
    assert_eq!(cache.periods[0].snapshot.fingerprint, before.fingerprint);
    assert_eq!(cache.periods[0].snapshot.generated_at, before.generated_at);
    assert!(cache
        .periods
        .iter()
        .all(|saved| saved.privacy_invalidated && saved.ai_summary.is_none()));
    // Re-including an identical record cannot make an older in-flight result current again.
    let (stale, _) = get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    assert!(stale.stale);
    assert_ne!(stale.current_fingerprint, before.fingerprint);
}

#[test]
fn retention_transition_changes_fingerprint_without_clock_tick_churn() {
    let now = date("2026-10-12T12:00:00Z");
    let mut config = settings();
    config.analytics_retention_days = 1;
    let records = vec![entry(
        "expiring",
        now - Duration::days(1) + Duration::minutes(1),
        "privatefixture privatefixture",
    )];
    let mut cache = TrainerCache::default();
    let (first, _) = get_from_cache(&mut cache, &records, 7, false, &config, now).unwrap();
    assert_eq!(first.snapshot.included_sessions, 1);
    let (expired, _) = get_from_cache(
        &mut cache,
        &records,
        7,
        false,
        &config,
        now + Duration::minutes(1),
    )
    .unwrap();
    assert!(expired.stale);
    assert_ne!(expired.current_fingerprint, first.current_fingerprint);
    assert!(expired.snapshot.terms.is_empty());
    assert!(eligible_sources(
        &records,
        Period::new(7, now).unwrap(),
        &config,
        now + Duration::minutes(1)
    )
    .is_empty());
}

#[test]
fn low_sample_guard_and_period_cache_are_bounded_and_rules_drift_requires_refresh() {
    let now = date("2026-10-12T12:00:00Z");
    let records = (0..3)
        .map(|index| entry(&index.to_string(), now, &"слово ".repeat(34)))
        .collect::<Vec<_>>();
    let mut cache = TrainerCache::default();
    for days in [7, 30, 90, 7, 30, 90] {
        let (report, _) =
            get_from_cache(&mut cache, &records, days, false, &settings(), now).unwrap();
        assert!(!report.snapshot.insufficient_data);
        assert_eq!(report.snapshot.history_limit, 200);
        assert_eq!(report.snapshot.total_words, 102);
        assert_eq!(report.snapshot.daily.len(), days as usize);
    }
    assert_eq!(cache.periods.len(), 3);
    cache.periods[0].snapshot.rules_version = "old-rules".into();
    let (stale, _) = get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    assert!(stale.stale);
    let (current, _) = get_from_cache(&mut cache, &records, 7, true, &settings(), now).unwrap();
    assert_eq!(current.snapshot.rules_version, RULES_VERSION);
    assert!(!current.stale);
}

#[test]
fn snapshot_and_ai_wire_models_roundtrip_with_the_frontend_camel_case_contract() {
    let now = date("2026-10-12T12:00:00Z");
    let records = vec![entry("fixture", now, "ну я я то есть...")];
    let mut cache = TrainerCache::default();
    let (local, _) = get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    let mut metrics_context = context();
    metrics_context.data_scope = SpeechLlmDataScope::MetricsOnly;
    let report = TrainerReport {
        snapshot: local.snapshot,
        current_fingerprint: local.current_fingerprint.clone(),
        stale: false,
        ai_fingerprint: "ai-fingerprint".into(),
        ai_stale: false,
        ai_summary: Some(summary(&local.current_fingerprint)),
        ai_configuration: TrainerAiConfiguration {
            enabled: true,
            profile_id: Some("fixture".into()),
            profile_name: Some("Fixture local".into()),
            model: Some("fixture-model".into()),
            cloud: false,
            cloud_consent: false,
            data_scope: SpeechLlmDataScope::MetricsOnly,
        },
        ai_disabled_reason: None,
        ai_context: Some(TrainerAiContextPreflight {
            metrics_only: metrics_context.clone(),
            configured_shared: Some(context()),
        }),
    };
    let json = serde_json::to_value(report).unwrap();
    assert!(json.get("currentFingerprint").is_some());
    assert!(json["snapshot"].get("rulesVersion").is_some());
    assert!(json["snapshot"]["counts"].get("selfCorrection").is_some());
    assert!(json["snapshot"]["terms"][0]
        .get("affectedSessions")
        .is_some());
    assert_eq!(json["aiConfiguration"]["dataScope"], "metrics_only");
    assert_eq!(json["aiSummary"]["dataScope"], "findings");
    let expected_context = serde_json::json!({
        "dataScope": "findings",
        "payloadBytes": 4321,
        "payloadByteLimit": 48000,
        "reduced": true,
        "metricsSessions": 13,
        "metricsWords": 500,
        "availableSessions": 13,
        "sharedSessions": 0,
        "omittedSessions": 13,
        "shortenedSessions": 0,
        "availableWords": 500,
        "sharedWords": 0,
        "omittedWords": 500,
        "availableCharacters": 2500,
        "sharedCharacters": 0,
        "omittedCharacters": 2500,
        "patternsAvailable": 43,
        "patternsShared": 40,
        "examplesAvailable": 120,
        "examplesShared": 80,
        "limits": {
            "maxSharedSessions": 12,
            "maxCharactersPerSession": 3000,
            "maxPatterns": 40,
            "maxExamplesPerPattern": 2,
        },
    });
    assert_eq!(json["aiSummary"]["context"], expected_context);
    assert_eq!(json["aiContext"]["configuredShared"], expected_context);
    assert_eq!(
        json["aiContext"]["metricsOnly"]["dataScope"],
        "metrics_only"
    );
    let decoded: TrainerReport = serde_json::from_value(json).unwrap();
    assert_eq!(decoded.snapshot.rules_version, RULES_VERSION);
    let decoded_preflight = decoded.ai_context.unwrap();
    assert_eq!(decoded_preflight.metrics_only, metrics_context);
    assert_eq!(decoded_preflight.configured_shared, Some(context()));
    assert_eq!(decoded.ai_summary.unwrap().context, Some(context()));
}

#[test]
fn legacy_reports_and_summaries_without_context_keep_the_saved_ai_result() {
    let now = date("2026-10-12T12:00:00Z");
    let records = vec![entry("fixture", now, "ну я я то есть...")];
    let mut cache = TrainerCache::default();
    let (local, _) = get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    let mut legacy_summary = serde_json::to_value(summary(&local.current_fingerprint)).unwrap();
    legacy_summary.as_object_mut().unwrap().remove("context");
    let decoded_summary: TrainerAiSummary = serde_json::from_value(legacy_summary.clone()).unwrap();
    assert!(decoded_summary.context.is_none());
    assert_eq!(decoded_summary.summary, "privatefixture coaching");
    assert_eq!(
        decoded_summary.snapshot_fingerprint,
        local.current_fingerprint
    );
    let legacy_report = serde_json::json!({
        "snapshot": local.snapshot,
        "currentFingerprint": local.current_fingerprint,
        "stale": false,
        "aiFingerprint": "ai-parameters",
        "aiStale": false,
        "aiSummary": legacy_summary,
        "aiConfiguration": {
            "enabled": true,
            "profileId": "fixture",
            "profileName": "Fixture local",
            "model": "fixture-model",
            "cloud": false,
            "cloudConsent": false,
            "dataScope": "metrics_only",
        },
        "aiDisabledReason": null,
    });
    let decoded_report: TrainerReport = serde_json::from_value(legacy_report).unwrap();
    assert!(decoded_report.ai_context.is_none());
    let saved_summary = decoded_report.ai_summary.unwrap();
    assert!(saved_summary.context.is_none());
    assert_eq!(saved_summary.fingerprint, "ai-parameters");
    assert_eq!(saved_summary.summary, "privatefixture coaching");
}

#[test]
fn delayed_ai_cannot_be_saved_after_a_source_change() {
    let now = date("2026-10-12T12:00:00Z");
    let mut records = vec![entry("a", now, "ну я я")];
    let mut cache = TrainerCache::default();
    let (requested, _) = get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    let delayed = summary(&requested.current_fingerprint);
    records.push(entry("b", now, "additional words"));
    let current = fingerprint_for(
        &records,
        Period::new(7, now).unwrap(),
        &settings(),
        now,
        cache.privacy_generation,
    )
    .unwrap();
    assert!(insert_ai(
        &mut cache,
        7,
        &requested.current_fingerprint,
        &current,
        delayed
    )
    .is_err());
    assert!(cache.periods[0].ai_summary.is_none());
    assert_eq!(cache.periods[0].snapshot.included_sessions, 1);
}

#[test]
fn reinclude_and_refresh_accepts_only_new_ai_provenance_and_persists_it() {
    let now = date("2026-10-12T12:00:00Z");
    let records = vec![entry("same-source", now, "ну я я")];
    let mut cache = TrainerCache::default();
    let (before_exclusion, _) =
        get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    let delayed = summary(&before_exclusion.current_fingerprint);
    sanitize_cache(&mut cache);
    // The original record is later re-included, and the user explicitly refreshes it.
    let (refreshed, _) = get_from_cache(&mut cache, &records, 7, true, &settings(), now).unwrap();
    assert_ne!(
        before_exclusion.current_fingerprint,
        refreshed.current_fingerprint
    );
    assert!(insert_ai(
        &mut cache,
        7,
        &before_exclusion.current_fingerprint,
        &refreshed.current_fingerprint,
        delayed
    )
    .is_err());
    insert_ai(
        &mut cache,
        7,
        &refreshed.current_fingerprint,
        &refreshed.current_fingerprint,
        summary(&refreshed.current_fingerprint),
    )
    .unwrap();
    let persisted = serde_json::to_vec(&cache).unwrap();
    let mut reopened: TrainerCache = serde_json::from_slice(&persisted).unwrap();
    let (report, written) = get_from_cache(
        &mut reopened,
        &records,
        7,
        false,
        &settings(),
        now + Duration::minutes(1),
    )
    .unwrap();
    assert!(!written);
    assert!(!report.stale);
    let saved_summary = report.ai_summary.unwrap();
    assert_eq!(
        saved_summary.snapshot_fingerprint,
        refreshed.current_fingerprint
    );
    assert_eq!(saved_summary.context, Some(context()));
}

#[test]
fn refresh_of_a_fresh_persisted_report_preserves_generation_time_and_ai_result() {
    let now = date("2026-10-12T12:00:00Z");
    let records = vec![entry("fixture", now, "ну я я")];
    let mut cache = TrainerCache::default();
    let (first, _) = get_from_cache(&mut cache, &records, 7, false, &settings(), now).unwrap();
    insert_ai(
        &mut cache,
        7,
        &first.current_fingerprint,
        &first.current_fingerprint,
        summary(&first.current_fingerprint),
    )
    .unwrap();
    let persisted = serde_json::to_vec(&cache).unwrap();
    let mut reopened: TrainerCache = serde_json::from_slice(&persisted).unwrap();
    let (refreshed, written) = get_from_cache(
        &mut reopened,
        &records,
        7,
        true,
        &settings(),
        now + Duration::hours(1),
    )
    .unwrap();
    assert!(!written);
    assert!(!refreshed.stale);
    assert_eq!(refreshed.snapshot.generated_at, first.snapshot.generated_at);
    assert_eq!(refreshed.snapshot.fingerprint, first.current_fingerprint);
    assert_eq!(refreshed.ai_summary.unwrap().fingerprint, "ai-parameters");
    assert_eq!(serde_json::to_vec(&reopened).unwrap(), persisted);
}
