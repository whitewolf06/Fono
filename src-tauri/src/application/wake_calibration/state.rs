use super::rules::{
    profile_graph, WakeCalibrationRejection, WakeCalibrationSampleResult, CANDIDATES,
    REQUIRED_SAMPLES,
};
use crate::{
    error::{AppError, AppResult},
    types::{Settings, WakeCalibrationProfile},
};
use parking_lot::Mutex;

#[derive(Default)]
pub struct WakeCalibrationService {
    session: Mutex<Option<Session>>,
    next_id: Mutex<u64>,
}
struct Session {
    id: u64,
    settings: Settings,
    recording: bool,
    accepted: Vec<WakeCalibrationSampleResult>,
    candidate_passes: [u8; 5],
    rejected: u8,
    latest: Option<WakeCalibrationSampleResult>,
    latest_hits: [bool; 5],
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeCalibrationStatus {
    pub active: bool,
    pub recording: bool,
    pub required_samples: u8,
    pub accepted_samples: u8,
    pub rejected_samples: u8,
    pub phrase: String,
    pub latest_result: Option<WakeCalibrationSampleResult>,
    pub profile: Option<WakeCalibrationProfile>,
}
impl WakeCalibrationService {
    pub(super) fn begin(&self, settings: Settings) -> AppResult<()> {
        let mut id = self.next_id.lock();
        let mut session = self.session.lock();
        if session.as_ref().is_some_and(|s| s.recording) {
            return Err(AppError::Audio("Дождитесь завершения записи".into()));
        }
        *id = id.wrapping_add(1).max(1);
        *session = Some(Session {
            id: *id,
            settings,
            recording: false,
            accepted: Vec::new(),
            candidate_passes: [0; 5],
            rejected: 0,
            latest: None,
            latest_hits: [false; 5],
        });
        Ok(())
    }
    pub(super) fn begin_recording(&self) -> AppResult<(u64, Settings)> {
        let mut session = self.session.lock();
        let session = session
            .as_mut()
            .ok_or_else(|| AppError::Config("Сначала начните настройку фразы".into()))?;
        if session.recording {
            return Err(AppError::Audio("Уже записывается образец".into()));
        }
        if session.accepted.len() >= REQUIRED_SAMPLES as usize {
            return Err(AppError::Config("Настройка уже завершена".into()));
        }
        session.recording = true;
        Ok((session.id, session.settings.clone()))
    }
    pub(super) fn failed(&self, id: u64) {
        if let Some(s) = self.session.lock().as_mut().filter(|s| s.id == id) {
            s.recording = false;
        }
    }
    pub(super) fn record(
        &self,
        id: u64,
        mut result: WakeCalibrationSampleResult,
        hits: [bool; 5],
    ) -> Option<WakeCalibrationProfile> {
        let mut sessions = self.session.lock();
        let s = sessions.as_mut().filter(|s| s.id == id)?;
        s.recording = false;
        let previous = s.accepted.len() as u8;
        if result.accepted
            && !hits
                .iter()
                .enumerate()
                .any(|(i, hit)| *hit && s.candidate_passes[i] == previous)
        {
            result.accepted = false;
            result.reason = Some(WakeCalibrationRejection::PhraseNotDetected);
        }
        s.latest = Some(result.clone());
        if !result.accepted {
            s.rejected = s.rejected.saturating_add(1);
            return None;
        }
        s.accepted.push(result);
        s.latest_hits = hits;
        for (index, hit) in hits.iter().enumerate() {
            s.candidate_passes[index] += u8::from(*hit);
        }
        if s.accepted.len() != REQUIRED_SAMPLES as usize {
            return None;
        }
        // Maximal stability for ASR and maximal posterior threshold for KWS
        // that passed every tuning sample. Never claim aggregate RMS trains KWS.
        let index = s
            .candidate_passes
            .iter()
            .rposition(|&count| count == REQUIRED_SAMPLES)?;
        let count = s.accepted.len();
        let threshold = CANDIDATES[index];
        Some(WakeCalibrationProfile {
            backend: s.settings.wake_backend,
            model_version: fono_wake::model_version(s.settings.wake_backend).into(),
            phrase: s.settings.wake_word.clone(),
            graph: profile_graph(&s.settings),
            threshold,
            sensitivity: s.settings.wake_word_sensitivity,
            vad_threshold: s.settings.wake_word_vad_threshold,
            completed_at: chrono::Utc::now(),
            accepted_samples: REQUIRED_SAMPLES,
            rejected_samples: s.rejected,
            average_rms: s.accepted.iter().map(|r| r.rms).sum::<f32>() / count as f32,
            average_peak: s.accepted.iter().map(|r| r.peak).sum::<f32>() / count as f32,
            average_active_ms: s.accepted.iter().map(|r| r.active_ms).sum::<u64>() / count as u64,
            validation: None,
        })
    }
    pub(super) fn retry_final_sample(&self, id: u64) {
        let mut sessions = self.session.lock();
        if let Some(s) = sessions.as_mut().filter(|s| s.id == id) {
            if s.accepted.len() == REQUIRED_SAMPLES as usize {
                s.accepted.pop();
                for (index, hit) in s.latest_hits.iter().enumerate() {
                    s.candidate_passes[index] -= u8::from(*hit);
                }
                s.latest = None;
            }
        }
    }
    pub(super) fn cancel(&self) -> AppResult<()> {
        let mut sessions = self.session.lock();
        if sessions.as_ref().is_some_and(|s| s.recording) {
            return Err(AppError::Audio("Дождитесь завершения записи".into()));
        }
        *sessions = None;
        Ok(())
    }
    pub(super) fn status(&self, profile: Option<WakeCalibrationProfile>) -> WakeCalibrationStatus {
        let session = self.session.lock();
        let s = session.as_ref();
        WakeCalibrationStatus {
            active: s.is_some_and(|s| s.accepted.len() < REQUIRED_SAMPLES as usize),
            recording: s.is_some_and(|s| s.recording),
            required_samples: REQUIRED_SAMPLES,
            accepted_samples: s.map_or(0, |s| s.accepted.len() as u8),
            rejected_samples: s.map_or(0, |s| s.rejected),
            phrase: s.map_or_else(
                || profile.as_ref().map_or(String::new(), |p| p.phrase.clone()),
                |s| s.settings.wake_word.clone(),
            ),
            latest_result: s.and_then(|s| s.latest.clone()),
            profile,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> WakeCalibrationSampleResult {
        WakeCalibrationSampleResult {
            accepted: true,
            detected: true,
            matched_candidates: 2,
            reason: None,
            rms: 0.1,
            peak: 0.4,
            active_ms: 700,
        }
    }
    #[test]
    fn tuning_selects_strictest_candidate_that_passed_every_sample() {
        let service = WakeCalibrationService::default();
        let settings = Settings::default();
        service.begin(settings).unwrap();
        for index in 0..5 {
            let (id, _) = service.begin_recording().unwrap();
            let result = service.record(id, sample(), [true, true, true, false, false]);
            if index < 4 {
                assert!(result.is_none());
            } else {
                let profile = result.unwrap();
                assert_eq!(profile.threshold, 0.5);
                assert_eq!(profile.accepted_samples, 5);
                assert!(profile.validation.is_none());
            }
        }
    }
    #[test]
    fn inconsistent_sample_is_rejected_without_destroying_viable_tuning() {
        let service = WakeCalibrationService::default();
        service.begin(Settings::default()).unwrap();
        let (id, _) = service.begin_recording().unwrap();
        service.record(id, sample(), [true, false, false, false, false]);
        let (id, _) = service.begin_recording().unwrap();
        service.record(id, sample(), [false, true, false, false, false]);
        let status = service.status(None);
        assert_eq!(status.accepted_samples, 1);
        assert_eq!(status.rejected_samples, 1);
        assert_eq!(
            status.latest_result.unwrap().reason,
            Some(WakeCalibrationRejection::PhraseNotDetected)
        );
    }
    #[test]
    fn failed_profile_save_can_retry_the_last_sample() {
        let service = WakeCalibrationService::default();
        service.begin(Settings::default()).unwrap();
        let mut final_id = 0;
        for _ in 0..REQUIRED_SAMPLES {
            let (id, _) = service.begin_recording().unwrap();
            final_id = id;
            service.record(id, sample(), [true, true, true, false, false]);
        }
        service.retry_final_sample(final_id);
        assert_eq!(service.status(None).accepted_samples, 4);
        let (id, _) = service.begin_recording().unwrap();
        let profile = service
            .record(id, sample(), [true, true, false, false, false])
            .unwrap();
        assert_eq!(profile.threshold, 0.3);
    }
    #[test]
    fn a_repeated_start_cannot_replace_a_recording_session() {
        let service = WakeCalibrationService::default();
        service.begin(Settings::default()).unwrap();
        let (id, _) = service.begin_recording().unwrap();
        assert!(service.begin(Settings::default()).is_err());
        service.failed(id);
        assert!(!service.status(None).recording);
    }
}
