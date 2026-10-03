//! Merge a writer's changed fields onto current settings while retaining one
//! lock through persistence and publication. Unrelated concurrent writers must
//! never publish the entire stale snapshot they started from.
use super::{save_settings, AppState};
use crate::{error::AppResult, types::Settings};
use std::sync::atomic::Ordering;

impl AppState {
    pub fn persist_settings_delta(
        &self,
        base: &Settings,
        candidate: &Settings,
    ) -> AppResult<Settings> {
        self.persist_settings_delta_with(base, candidate, save_settings)
    }

    fn persist_settings_delta_with(
        &self,
        base: &Settings,
        candidate: &Settings,
        persist: impl FnOnce(&Settings) -> AppResult<()>,
    ) -> AppResult<Settings> {
        let mut current = self.settings.lock();
        let merged = merge_delta(&current, base, candidate)?;
        // Readers and subsequent writers only see the new settings after disk
        // accepts them. A failed write leaves both memory and version intact.
        persist(&merged)?;
        *current = merged.clone();
        self.settings_version.fetch_add(1, Ordering::SeqCst);
        Ok(merged)
    }
}

fn merge_delta(latest: &Settings, base: &Settings, candidate: &Settings) -> AppResult<Settings> {
    let base_json = serde_json::to_value(base)?;
    let candidate_json = serde_json::to_value(candidate)?;
    let mut latest_json = serde_json::to_value(latest)?;
    let base_fields = base_json
        .as_object()
        .expect("Settings serializes as object");
    let candidate_fields = candidate_json
        .as_object()
        .expect("Settings serializes as object");
    let latest_fields = latest_json
        .as_object_mut()
        .expect("Settings serializes as object");
    for key in base_fields.keys().chain(candidate_fields.keys()) {
        if base_fields.get(key) == candidate_fields.get(key) {
            continue;
        }
        if let Some(value) = candidate_fields.get(key) {
            latest_fields.insert(key.clone(), value.clone());
        } else {
            latest_fields.remove(key);
        }
    }
    let mut merged: Settings = serde_json::from_value(latest_json)?;
    // Serialization deliberately excludes secrets and one legacy field. Their
    // in-memory values still obey the same delta rule, without putting them in
    // JSON or exposing them to callers of the settings IPC.
    merged.use_gpu = changed_value(base.use_gpu, candidate.use_gpu, latest.use_gpu);
    merged.llm_api_key = changed_value(
        base.llm_api_key.clone(),
        candidate.llm_api_key.clone(),
        latest.llm_api_key.clone(),
    );
    for profile in &mut merged.llm_profiles {
        let old = base.llm_profiles.iter().find(|item| item.id == profile.id);
        let next = candidate
            .llm_profiles
            .iter()
            .find(|item| item.id == profile.id);
        let current = latest
            .llm_profiles
            .iter()
            .find(|item| item.id == profile.id);
        profile.api_key = if old.map(|item| &item.api_key) != next.map(|item| &item.api_key) {
            next.and_then(|item| item.api_key.clone())
        } else {
            current.and_then(|item| item.api_key.clone())
        };
    }
    merged.enforce_classic_dictation();
    Ok(merged)
}

fn changed_value<T: PartialEq>(base: T, candidate: T, latest: T) -> T {
    if base != candidate {
        candidate
    } else {
        latest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::AppError, types::DictationMode};
    use parking_lot::Mutex;
    use std::sync::{atomic::AtomicU64, Arc, Barrier};

    fn isolated_state(settings: Settings) -> AppState {
        AppState {
            settings: Mutex::new(settings),
            pipeline_state: Mutex::new(crate::types::PipelineState::Idle),
            dictation_paused: Mutex::new(false),
            settings_version: AtomicU64::new(1),
            next_command_proposal_id: AtomicU64::new(1),
            pending_command_proposal: Mutex::new(None),
        }
    }

    #[test]
    fn simultaneous_overlay_and_update_setting_writers_retain_both_changes() {
        let state = Arc::new(isolated_state(Settings::default()));
        let persisted = Arc::new(Mutex::new(Settings::default()));
        let snapshots_taken = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|writer| {
                let state = state.clone();
                let persisted = persisted.clone();
                let snapshots_taken = snapshots_taken.clone();
                std::thread::spawn(move || {
                    let base = state.settings();
                    let mut candidate = base.clone();
                    if writer == 0 {
                        candidate.update_checks_enabled = true;
                    } else {
                        candidate.overlay_x = Some(147);
                        candidate.overlay_y = Some(283);
                    }
                    snapshots_taken.wait();
                    state
                        .persist_settings_delta_with(&base, &candidate, |merged| {
                            *persisted.lock() = merged.clone();
                            Ok(())
                        })
                        .expect("save a disjoint settings delta");
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("writer completes");
        }
        for settings in [state.settings(), persisted.lock().clone()] {
            assert!(settings.update_checks_enabled);
            assert_eq!(settings.overlay_x, Some(147));
            assert_eq!(settings.overlay_y, Some(283));
        }
        assert_eq!(state.settings_version.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn failed_persistence_keeps_latest_memory_disk_and_version_unchanged() {
        let base = Settings::default();
        let mut latest = base.clone();
        latest.update_checks_enabled = true;
        let state = isolated_state(latest.clone());
        let mut candidate = base.clone();
        candidate.overlay_x = Some(13);
        let persisted = Mutex::new(latest);
        let result = state.persist_settings_delta_with(&base, &candidate, |merged| {
            assert!(merged.update_checks_enabled);
            assert_eq!(merged.overlay_x, Some(13));
            Err(AppError::Config("injected persistence failure".into()))
        });
        assert!(result.is_err());
        assert!(state.settings().update_checks_enabled);
        assert_eq!(state.settings().overlay_x, None);
        assert_eq!(persisted.lock().overlay_x, None);
        assert_eq!(state.settings_version.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn delta_removes_optional_fields_and_never_reenables_live_mode() {
        let mut base = Settings::default();
        base.wake_calibration_profile = Some(crate::types::WakeCalibrationProfile {
            backend: base.wake_backend,
            model_version: "test".into(),
            phrase: "test".into(),
            graph: "test".into(),
            threshold: 0.2,
            sensitivity: 0.3,
            vad_threshold: 0.4,
            completed_at: chrono::Utc::now(),
            accepted_samples: 5,
            rejected_samples: 0,
            average_rms: 0.1,
            average_peak: 0.2,
            average_active_ms: 100,
            validation: None,
        });
        let mut candidate = base.clone();
        candidate.wake_calibration_profile = None;
        candidate.dictation_mode = DictationMode::Live;
        let mut latest = base.clone();
        latest.service_enabled = false;
        let merged = merge_delta(&latest, &base, &candidate).expect("merge fields");
        assert!(merged.wake_calibration_profile.is_none());
        assert_eq!(merged.dictation_mode, DictationMode::Standard);
        assert!(!merged.service_enabled);
    }

    #[test]
    fn nonserialized_fields_survive_unrelated_writes_and_explicit_removal() {
        let base = Settings::default();
        let mut latest = base.clone();
        latest.use_gpu = !base.use_gpu;
        latest.llm_api_key = Some("test legacy secret".into());
        latest.llm_profiles[0].api_key = Some("test profile secret".into());
        let mut candidate = base.clone();
        candidate.overlay_y = Some(21);
        let merged = merge_delta(&latest, &base, &candidate).expect("merge without secrets loss");
        assert_eq!(merged.use_gpu, latest.use_gpu);
        assert_eq!(merged.llm_api_key, latest.llm_api_key);
        assert_eq!(
            merged.llm_profiles[0].api_key,
            latest.llm_profiles[0].api_key
        );
        let json = serde_json::to_string(&merged).expect("serialize without secrets");
        assert!(!json.contains("test legacy secret"));
        assert!(!json.contains("test profile secret"));
        let mut candidate = latest.clone();
        candidate.llm_api_key = None;
        candidate.llm_profiles[0].api_key = None;
        let merged = merge_delta(&latest, &latest, &candidate).expect("remove private fields");
        assert!(merged.llm_api_key.is_none());
        assert!(merged.llm_profiles[0].api_key.is_none());
    }
}
