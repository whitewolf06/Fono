use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::types::WakeCalibrationValidation;

use super::rules::{
    kind_complete, WakeProfileValidationKind, WakeProfileValidationSampleResult,
    WakeProfileValidationStatus, NEGATIVE_REQUIRED, POSITIVE_REQUIRED,
};

#[derive(Default)]
pub struct WakeProfileValidationService {
    session: Mutex<Option<ValidationSession>>,
    next_session_id: Mutex<u64>,
}

pub(super) struct ValidationSession {
    pub(super) id: u64,
    pub(super) threshold: f32,
    pub(super) recording: bool,
    pub(super) failed: bool,
    pub(super) positive_passed: u8,
    pub(super) silence_passed: bool,
    pub(super) other_phrase_passed: bool,
    pub(super) latest_result: Option<WakeProfileValidationSampleResult>,
    profile_completed_at: Option<chrono::DateTime<chrono::Utc>>,
    profile_graph: Option<String>,
}

impl WakeProfileValidationService {
    pub(super) fn cancel(&self) -> AppResult<()> {
        let mut sessions = self.session.lock();
        if sessions.as_ref().is_some_and(|s| s.recording) {
            return Err(AppError::Audio(
                "Дождитесь завершения проверочной записи".into(),
            ));
        }
        *sessions = None;
        Ok(())
    }
    pub(super) fn begin(&self, threshold: f32) {
        let mut next_session_id = self.next_session_id.lock();
        *next_session_id = next_session_id.wrapping_add(1).max(1);
        *self.session.lock() = Some(ValidationSession {
            id: *next_session_id,
            threshold,
            recording: false,
            failed: false,
            positive_passed: 0,
            silence_passed: false,
            other_phrase_passed: false,
            latest_result: None,
            profile_completed_at: None,
            profile_graph: None,
        });
    }

    pub(super) fn bind_profile(&self, profile: &crate::types::WakeCalibrationProfile) {
        if let Some(s) = self.session.lock().as_mut() {
            s.profile_completed_at = Some(profile.completed_at);
            s.profile_graph = Some(profile.graph.clone());
        }
    }
    pub(super) fn matches_profile(&self, profile: &crate::types::WakeCalibrationProfile) -> bool {
        self.session.lock().as_ref().is_some_and(|s| {
            s.profile_completed_at == Some(profile.completed_at)
                && s.profile_graph.as_deref() == Some(profile.graph.as_str())
                && (s.threshold - profile.threshold).abs() < f32::EPSILON
        })
    }

    pub(super) fn begin_recording(&self, kind: WakeProfileValidationKind) -> AppResult<u64> {
        let mut sessions = self.session.lock();
        let session = sessions
            .as_mut()
            .ok_or_else(|| AppError::Config("Сначала начните проверку профиля".into()))?;
        if session.recording {
            return Err(AppError::Audio(
                "Уже записывается проверочный образец".into(),
            ));
        }
        if session.failed {
            return Err(AppError::Config(
                "Проверка не пройдена. Запустите её заново или повторите калибровку".into(),
            ));
        }
        if kind_complete(session, kind) {
            return Err(AppError::Config(
                "Этот тип проверочного образца уже принят".into(),
            ));
        }
        session.recording = true;
        Ok(session.id)
    }

    pub(super) fn recording_failed(&self, id: u64) {
        let mut sessions = self.session.lock();
        if let Some(session) = sessions.as_mut().filter(|session| session.id == id) {
            session.recording = false;
        }
    }

    pub(super) fn record_result(
        &self,
        id: u64,
        result: WakeProfileValidationSampleResult,
    ) -> Option<WakeCalibrationValidation> {
        let mut sessions = self.session.lock();
        let session = sessions.as_mut().filter(|session| session.id == id)?;
        session.recording = false;
        session.latest_result = Some(result.clone());
        if result.input_issue.is_some() {
            return None;
        }
        if !result.accepted {
            session.failed = true;
            return None;
        }
        match result.kind {
            WakeProfileValidationKind::Positive => {
                session.positive_passed = session.positive_passed.saturating_add(1);
            }
            WakeProfileValidationKind::Silence => session.silence_passed = true,
            WakeProfileValidationKind::OtherPhrase => session.other_phrase_passed = true,
        }
        if session.positive_passed == POSITIVE_REQUIRED
            && session.silence_passed
            && session.other_phrase_passed
        {
            let validation = WakeCalibrationValidation {
                completed_at: chrono::Utc::now(),
                positive_passed: session.positive_passed,
                positive_required: POSITIVE_REQUIRED,
                negative_passed: NEGATIVE_REQUIRED,
                negative_required: NEGATIVE_REQUIRED,
                confirmed_threshold: session.threshold,
            };
            *sessions = None;
            return Some(validation);
        }
        None
    }

    pub(super) fn status(&self, completed: bool) -> WakeProfileValidationStatus {
        let session = self.session.lock();
        let Some(session) = session.as_ref() else {
            return default_status(completed);
        };
        WakeProfileValidationStatus {
            active: !session.failed,
            recording: session.recording,
            failed: session.failed,
            completed,
            positive_passed: session.positive_passed,
            positive_required: POSITIVE_REQUIRED,
            silence_passed: session.silence_passed,
            other_phrase_passed: session.other_phrase_passed,
            negative_required: NEGATIVE_REQUIRED,
            latest_result: session.latest_result.clone(),
        }
    }
}

fn default_status(completed: bool) -> WakeProfileValidationStatus {
    WakeProfileValidationStatus {
        active: false,
        recording: false,
        failed: false,
        completed,
        positive_passed: 0,
        positive_required: POSITIVE_REQUIRED,
        silence_passed: false,
        other_phrase_passed: false,
        negative_required: NEGATIVE_REQUIRED,
        latest_result: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::wake_validation::rules::expects_detection;

    fn accepted(kind: WakeProfileValidationKind) -> WakeProfileValidationSampleResult {
        WakeProfileValidationSampleResult {
            kind,
            detected: expects_detection(kind),
            accepted: true,
            input_issue: None,
        }
    }

    #[test]
    fn validation_requires_all_three_positive_and_two_negative_checks() {
        let service = WakeProfileValidationService::default();
        service.begin(0.25);
        for _ in 0..POSITIVE_REQUIRED {
            let id = service
                .begin_recording(WakeProfileValidationKind::Positive)
                .unwrap();
            assert!(service
                .record_result(id, accepted(WakeProfileValidationKind::Positive))
                .is_none());
        }
        let silence_id = service
            .begin_recording(WakeProfileValidationKind::Silence)
            .unwrap();
        assert!(service
            .record_result(silence_id, accepted(WakeProfileValidationKind::Silence))
            .is_none());
        let other_id = service
            .begin_recording(WakeProfileValidationKind::OtherPhrase)
            .unwrap();
        let completed = service
            .record_result(other_id, accepted(WakeProfileValidationKind::OtherPhrase))
            .expect("the final check completes validation");

        assert_eq!(completed.positive_passed, POSITIVE_REQUIRED);
        assert_eq!(completed.negative_passed, NEGATIVE_REQUIRED);
        assert_eq!(completed.confirmed_threshold, 0.25);
    }
}
