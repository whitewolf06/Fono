//! Desktop adapter for the shared recognition engine.

#[cfg(test)]
use crate::error::AppError;
use crate::error::AppResult;
use crate::types::{AccelerationMode, SttModelSelection, Transcript};
use fono_core::OperationCancellation;
use fono_stt::ports::{ActivityGuard, ActivityLease, SttEnvironment};
use std::{path::Path, sync::Arc};

mod paths;
pub use fono_stt::{
    CancellationSupport, GpuResidency, PreparedRecoverySelection, RecognizerCapabilities,
    RecoverySelection, RecoverySelectionProvider, SttHealth, SttReadiness, SttReadinessObserver,
    TimedSegment, WindowTranscript, WorkerPaths,
};
pub use paths::worker_paths_for_app;

pub mod offline_models {
    pub use fono_stt::offline_models::{OfflineArtifact, OfflineModelSpec};
    pub fn specifications() -> crate::error::AppResult<Vec<OfflineModelSpec>> {
        fono_stt::offline_models::specifications().map_err(Into::into)
    }
    pub fn specification(id: &crate::types::ModelId) -> crate::error::AppResult<OfflineModelSpec> {
        fono_stt::offline_models::specification(id).map_err(Into::into)
    }
}

#[derive(Default)]
pub(crate) struct DesktopActivity;
impl ActivityGuard for DesktopActivity {
    fn lease(&self) -> fono_stt::error::SttResult<ActivityLease> {
        crate::application::updates::activity::lease()
            .map(ActivityLease::new)
            .map_err(Into::into)
    }
    fn cleanup_lease(&self) -> ActivityLease {
        ActivityLease::new(crate::application::updates::activity::cleanup_lease())
    }
}

pub(crate) fn probe_cuda_worker(
    package: &crate::application::cuda_packages::VerifiedPackage,
) -> AppResult<()> {
    fono_stt::probe_cuda_worker(
        fono_stt::ports::GpuPackageHandle::new(package.clone()),
        Arc::new(DesktopActivity),
    )
    .map_err(Into::into)
}

pub struct SttEngine {
    inner: Arc<fono_stt::SttEngine>,
}
impl Default for SttEngine {
    fn default() -> Self {
        Self::new()
    }
}
impl std::ops::Deref for SttEngine {
    type Target = fono_stt::SttEngine;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl SttEngine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(fono_stt::SttEngine::new(SttEnvironment {
                activity: Arc::new(DesktopActivity),
                error_message: Arc::new(crate::error::stt_user_message),
                ..SttEnvironment::default()
            })),
        }
    }
    pub fn ensure_loaded(
        &self,
        model_path: &Path,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
    ) -> AppResult<()> {
        self.inner
            .ensure_loaded(model_path, acceleration, worker_paths)
            .map_err(Into::into)
    }

    pub fn ensure_model_loaded(
        &self,
        model: &SttModelSelection,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        self.inner
            .ensure_model_loaded_with_paths(model, acceleration, worker_paths, cancellation)
            .map_err(Into::into)
    }

    pub fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.inner.transcribe(samples, language).map_err(Into::into)
    }

    pub fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
        self.inner
            .transcribe_cancellable(samples, language, cancellation)
            .map_err(Into::into)
    }

    pub fn transcribe_with_options(
        &self,
        samples: &[i16],
        language: &str,
        decoding: fono_stt_protocol::DecodingOptions,
    ) -> AppResult<Transcript> {
        self.inner
            .transcribe_with_options(samples, language, decoding)
            .map_err(Into::into)
    }

    pub fn transcribe_cancellable_with_options(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        decoding: fono_stt_protocol::DecodingOptions,
    ) -> AppResult<Transcript> {
        self.inner
            .transcribe_cancellable_with_options(samples, language, cancellation, decoding)
            .map_err(Into::into)
    }

    pub fn transcribe_cancellable_with_glossary(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: &[String],
    ) -> AppResult<Transcript> {
        self.inner
            .transcribe_cancellable_with_glossary(
                samples,
                language,
                cancellation,
                decoding,
                glossary,
            )
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transcribe_window(
        &self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        operation_id: u64,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
    ) -> AppResult<WindowTranscript> {
        self.inner
            .transcribe_window(
                samples,
                language,
                context,
                operation_id,
                cancellation,
                audio_start_sample,
            )
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transcribe_window_with_options(
        &self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        operation_id: u64,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
        decoding: fono_stt_protocol::DecodingOptions,
    ) -> AppResult<WindowTranscript> {
        self.inner
            .transcribe_window_with_options(
                samples,
                language,
                context,
                operation_id,
                cancellation,
                audio_start_sample,
                decoding,
            )
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transcribe_window_with_glossary(
        &self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        operation_id: u64,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: &[String],
    ) -> AppResult<WindowTranscript> {
        self.inner
            .transcribe_window_with_glossary(
                samples,
                language,
                context,
                operation_id,
                cancellation,
                audio_start_sample,
                decoding,
                glossary,
            )
            .map_err(Into::into)
    }

    pub fn enable_recovery(
        self: &Arc<Self>,
        selection: RecoverySelectionProvider,
    ) -> AppResult<()> {
        self.inner.enable_recovery(selection).map_err(Into::into)
    }
}

#[cfg(all(test, windows))]
mod lifecycle_soak;

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn missing_model_preserves_desktop_ipc_and_readiness_guidance() {
        let engine = SttEngine::new();
        let error = engine
            .ensure_loaded(
                Path::new("missing-model-for-readiness-test.bin"),
                AccelerationMode::Cpu,
                &WorkerPaths::default(),
            )
            .unwrap_err();
        assert!(matches!(error, AppError::ModelFileMissing));
        assert_eq!(
            serde_json::to_value(error).unwrap()["code"],
            "model_missing"
        );
        assert!(
            matches!(engine.readiness(), SttReadiness::Failed { message }
            if message.contains("Файл выбранной модели не найден")
                && !message.contains("missing-model-for-readiness-test.bin"))
        );
    }
}
