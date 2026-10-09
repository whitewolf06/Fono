//! Process-local numeric measurements. No user content is accepted by this API.
use fono_core::OperationSource;
use parking_lot::Mutex;
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportOutcome {
    Completed,
    Empty,
    Cancelled,
    AudioError,
    RecognitionError,
    ProcessingError,
    InsertionError,
    InternalError,
    Interrupted,
}
impl ReportOutcome {
    pub fn from_error(error: &crate::error::AppError) -> Self {
        use crate::error::AppError;
        match error {
            AppError::Cancelled(_) => Self::Cancelled,
            AppError::Audio(_) => Self::AudioError,
            AppError::Stt(_) | AppError::ModelNotLoaded => Self::RecognitionError,
            AppError::Llm(_) => Self::ProcessingError,
            AppError::Injection(_) => Self::InsertionError,
            _ => Self::InternalError,
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Completed => "завершено",
            Self::Empty => "нет распознанного текста",
            Self::Cancelled => "отменено",
            Self::AudioError => "ошибка аудио",
            Self::RecognitionError => "ошибка распознавания",
            Self::ProcessingError => "ошибка обработки текста",
            Self::InsertionError => "ошибка вставки",
            Self::InternalError => "внутренняя ошибка",
            Self::Interrupted => "прервано до завершения",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ReportStage {
    CaptureFinish,
    ModelLoad,
    Recognition,
    Processing,
    Insertion,
}

#[derive(Debug, Clone)]
pub(super) struct DictationMeasurement {
    pub operation: u64,
    pub source: OperationSource,
    pub outcome: ReportOutcome,
    pub audio_ms: Option<u64>,
    pub finish_ms: u64,
    pub capture_finish_ms: Option<u64>,
    pub model_load_ms: Option<u64>,
    pub recognition_ms: Option<u64>,
    pub processing_ms: Option<u64>,
    pub insertion_ms: Option<u64>,
}
impl DictationMeasurement {
    fn set_stage(&mut self, stage: ReportStage, duration: Duration) {
        let target = match stage {
            ReportStage::CaptureFinish => &mut self.capture_finish_ms,
            ReportStage::ModelLoad => &mut self.model_load_ms,
            ReportStage::Recognition => &mut self.recognition_ms,
            ReportStage::Processing => &mut self.processing_ms,
            ReportStage::Insertion => &mut self.insertion_ms,
        };
        *target = Some(duration_ms(duration));
    }
}

#[derive(Default)]
struct ReportMeasurements {
    newest_started: u64,
    last: Option<DictationMeasurement>,
}
impl ReportMeasurements {
    fn started(&mut self, operation: u64) {
        self.newest_started = self.newest_started.max(operation);
    }
    fn complete(&mut self, measurement: DictationMeasurement) {
        // A replaced or cancelled operation may finish late on a worker thread.
        if measurement.operation >= self.newest_started {
            self.last = Some(measurement);
        }
    }
}
fn measurements() -> &'static Mutex<ReportMeasurements> {
    static STORE: OnceLock<Mutex<ReportMeasurements>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(ReportMeasurements::default()))
}

/// Created once by the ordinary dictation's Stop owner. Stores only the most
/// recent completed flow in memory, never in settings, history or log files.
pub struct ReportTrace {
    started: Instant,
    data: Arc<Mutex<DictationMeasurement>>,
}
impl ReportTrace {
    pub fn begin(operation: u64, source: OperationSource) -> Self {
        measurements().lock().started(operation);
        Self {
            started: Instant::now(),
            data: Arc::new(Mutex::new(DictationMeasurement {
                operation,
                source,
                outcome: ReportOutcome::Interrupted,
                audio_ms: None,
                finish_ms: 0,
                capture_finish_ms: None,
                model_load_ms: None,
                recognition_ms: None,
                processing_ms: None,
                insertion_ms: None,
            })),
        }
    }
    pub fn stage(&self, stage: ReportStage) -> ReportStageTimer {
        ReportStageTimer {
            stage,
            started: Instant::now(),
            data: self.data.clone(),
        }
    }
    pub fn captured(&self, samples: usize) {
        self.data.lock().audio_ms = Some((samples as u64).saturating_mul(1000) / 16_000);
    }
    pub fn outcome(&self, outcome: ReportOutcome) {
        self.data.lock().outcome = outcome;
    }
    /// Snapshot immediately after final text generation, before insertion.
    pub fn history_timings(&self) -> crate::types::DictationTimingMeasurements {
        let data = self.data.lock();
        crate::types::DictationTimingMeasurements {
            recording_duration_ms: data.audio_ms,
            generation_duration_ms: Some(duration_ms(self.started.elapsed())),
            model_load_duration_ms: data.model_load_ms,
            processing_duration_ms: data.processing_ms,
        }
    }
}
impl Drop for ReportTrace {
    fn drop(&mut self) {
        let mut measurement = self.data.lock().clone();
        measurement.finish_ms = duration_ms(self.started.elapsed());
        measurements().lock().complete(measurement);
    }
}

pub struct ReportStageTimer {
    stage: ReportStage,
    started: Instant,
    data: Arc<Mutex<DictationMeasurement>>,
}
impl Drop for ReportStageTimer {
    fn drop(&mut self) {
        self.data
            .lock()
            .set_stage(self.stage, self.started.elapsed());
    }
}
pub(super) fn last_measurement() -> Option<DictationMeasurement> {
    measurements().lock().last.clone()
}
fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn measurement(operation: u64) -> DictationMeasurement {
        DictationMeasurement {
            operation,
            source: OperationSource::Hotkey,
            outcome: ReportOutcome::Completed,
            audio_ms: Some(5000),
            finish_ms: 300,
            capture_finish_ms: None,
            model_load_ms: None,
            recognition_ms: None,
            processing_ms: None,
            insertion_ms: None,
        }
    }
    #[test]
    fn late_replaced_operation_cannot_replace_latest_measurement() {
        let mut store = ReportMeasurements::default();
        store.started(1);
        store.started(2);
        store.complete(measurement(2));
        store.complete(measurement(1));
        assert_eq!(store.last.unwrap().operation, 2);
    }
    #[test]
    fn unfinished_new_operation_keeps_previous_completed_report() {
        let mut store = ReportMeasurements::default();
        store.started(1);
        store.complete(measurement(1));
        store.started(2);
        store.complete(measurement(1));
        assert_eq!(store.last.unwrap().operation, 1);
    }
    #[test]
    fn independently_measured_stages_do_not_overwrite_each_other() {
        let mut data = measurement(1);
        data.set_stage(ReportStage::ModelLoad, Duration::from_millis(500));
        data.set_stage(ReportStage::Recognition, Duration::from_millis(1200));
        assert_eq!(data.model_load_ms, Some(500));
        assert_eq!(data.recognition_ms, Some(1200));
        assert_eq!(data.processing_ms, None);
    }
}
