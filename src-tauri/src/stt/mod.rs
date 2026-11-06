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

/// Read only the dedicated OS credential; never serialize a bearer into settings.
pub(crate) fn remote_endpoint(
    remote: &crate::types::RemoteSttSettings,
) -> AppResult<fono_stt::remote::RemoteEndpoint> {
    let token =
        zeroize::Zeroizing::new(crate::secrets::load_stt_server_token()?.ok_or_else(|| {
            crate::error::AppError::Config("Fono Server credential is not configured".into())
        })?);
    let mut endpoint = fono_stt::remote::RemoteEndpoint::new(&remote.url, &token)?;
    if let Some(pem) = &remote.ca_pem {
        endpoint = endpoint.with_ca_pem(pem.as_bytes())?;
    }
    Ok(endpoint)
}

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
    preparation: parking_lot::Mutex<PreparationStatus>,
}

#[derive(Default)]
struct PreparationStatus {
    generation: u64,
    startup_failure: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct PreparationAttempt(u64);
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
            preparation: parking_lot::Mutex::new(PreparationStatus::default()),
        }
    }

    /// Startup failures belong to the selected source without replacing a
    /// backend during a failed settings transaction. Only closed UI messages
    /// enter this transient status; it is never persisted.
    pub fn readiness(&self) -> SttReadiness {
        match self.preparation.lock().startup_failure.as_ref() {
            Some(message) => SttReadiness::Failed {
                message: message.clone(),
            },
            None => self.inner.readiness(),
        }
    }

    pub(crate) fn report_remote_startup_failure(
        &self,
        attempt: PreparationAttempt,
        error: &crate::error::AppError,
        publish: impl FnOnce(SttReadiness),
    ) -> Option<SttReadiness> {
        let message = error.user_message();
        let mut preparation = self.preparation.lock();
        if preparation.generation != attempt.0 {
            return None;
        }
        preparation.startup_failure = Some(message.clone());
        let readiness = SttReadiness::Failed { message };
        // The event and memory status share one fence. The callback only emits
        // the supplied event; it must not call back into this wrapper.
        publish(readiness.clone());
        Some(readiness)
    }

    fn begin_preparation(&self) -> PreparationAttempt {
        let mut preparation = self.preparation.lock();
        preparation.generation = preparation.generation.wrapping_add(1);
        PreparationAttempt(preparation.generation)
    }

    fn finish_preparation(&self, attempt: PreparationAttempt, successful: bool) {
        let mut preparation = self.preparation.lock();
        if successful && preparation.generation == attempt.0 {
            preparation.startup_failure = None;
        }
    }

    pub fn clear_remote_if_idle(&self) -> AppResult<()> {
        let attempt = self.begin_preparation();
        let result = self
            .inner
            .clear_remote_if_idle()
            .map_err(crate::error::AppError::from);
        self.finish_preparation(attempt, result.is_ok());
        result
    }
    pub fn ensure_loaded(
        &self,
        model_path: &Path,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
    ) -> AppResult<()> {
        let attempt = self.begin_preparation();
        let result = self
            .inner
            .ensure_loaded(model_path, acceleration, worker_paths)
            .map_err(Into::into);
        self.finish_preparation(attempt, result.is_ok());
        result
    }

    pub fn ensure_model_loaded(
        &self,
        model: &SttModelSelection,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        let attempt = self.begin_preparation();
        let result = self
            .inner
            .ensure_model_loaded_with_paths(model, acceleration, worker_paths, cancellation)
            .map_err(Into::into);
        self.finish_preparation(attempt, result.is_ok());
        result
    }

    /// Blocking host adapter. All callers retain scheduler/admission ownership.
    pub fn ensure_configured(
        &self,
        settings: &crate::types::Settings,
        worker_paths: &WorkerPaths,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        self.ensure_configured_with_attempt(settings, worker_paths, cancellation)
            .1
    }

    pub(crate) fn ensure_configured_with_attempt(
        &self,
        settings: &crate::types::Settings,
        worker_paths: &WorkerPaths,
        cancellation: &OperationCancellation,
    ) -> (PreparationAttempt, AppResult<()>) {
        let attempt = self.begin_preparation();
        let result = self.prepare_configured(settings, worker_paths, cancellation);
        self.finish_preparation(attempt, result.is_ok());
        (attempt, result)
    }

    fn prepare_configured(
        &self,
        settings: &crate::types::Settings,
        worker_paths: &WorkerPaths,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        if cancellation.is_cancelled() {
            return Err(crate::error::AppError::Cancelled(
                "STT preparation cancelled".into(),
            ));
        }
        match settings.stt_source {
            crate::types::SttSource::Local => {
                let Some(model) = settings.selected_stt_model() else {
                    return self.inner.clear_remote_if_idle().map_err(Into::into);
                };
                self.inner
                    .ensure_model_loaded_with_paths(
                        &model,
                        settings.acceleration,
                        worker_paths,
                        cancellation,
                    )
                    .map_err(Into::into)
            }
            crate::types::SttSource::FonoServer => {
                let remote = settings.remote_stt.as_ref().ok_or_else(|| {
                    crate::error::AppError::Config(
                        "Fono Server connection is not configured".into(),
                    )
                })?;
                let endpoint = remote_endpoint(remote)?;
                self.inner
                    .ensure_remote_loaded(endpoint, &remote.model, cancellation)
                    .map_err(Into::into)
            }
        }
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
    fn startup_error_remains_visible_to_late_reads_and_clears_on_source_retirement() {
        let engine = SttEngine::new();
        let attempt = engine.begin_preparation();
        let status = engine
            .report_remote_startup_failure(
                attempt,
                &AppError::Config("PRIVATE_ENDPOINT_TOKEN_CA_PATH".into()),
                |_| {},
            )
            .unwrap();
        assert!(matches!(&status, SttReadiness::Failed { message }
            if !message.contains("PRIVATE_") && !message.is_empty()));
        assert_eq!(
            serde_json::to_value(engine.readiness()).unwrap(),
            serde_json::to_value(status).unwrap()
        );
        let cancelled = OperationCancellation::default();
        cancelled.cancel();
        assert!(matches!(
            engine.ensure_configured(
                &crate::types::Settings::default(),
                &WorkerPaths::default(),
                &cancelled,
            ),
            Err(AppError::Cancelled(_))
        ));
        assert!(matches!(engine.readiness(), SttReadiness::Failed { .. }));
        engine
            .ensure_configured(
                &crate::types::Settings::default(),
                &WorkerPaths::default(),
                &OperationCancellation::default(),
            )
            .unwrap();
        assert!(matches!(engine.readiness(), SttReadiness::Unloaded));
    }

    #[test]
    fn startup_version_failure_preserves_the_update_server_action() {
        let engine = SttEngine::new();
        let error = AppError::from(fono_stt::error::SttError::Remote(
            fono_stt::remote::RemoteError {
                kind: fono_stt::remote::RemoteErrorKind::Incompatible,
                status: Some(404),
                retry_after_seconds: None,
            },
        ));
        let attempt = engine.begin_preparation();
        let readiness = engine
            .report_remote_startup_failure(attempt, &error, |_| {})
            .unwrap();
        assert!(matches!(readiness, SttReadiness::Failed { message }
            if message.contains("Обновите сервер") && !message.contains("404")));
    }

    #[test]
    fn late_startup_failure_cannot_replace_or_publish_after_a_new_success() {
        let engine = SttEngine::new();
        let old_attempt = engine.begin_preparation();
        engine
            .ensure_configured(
                &crate::types::Settings::default(),
                &WorkerPaths::default(),
                &OperationCancellation::default(),
            )
            .unwrap();
        assert!(engine
            .report_remote_startup_failure(
                old_attempt,
                &AppError::Config("PRIVATE_OLD_FAILURE".into()),
                |_| panic!("an old failure must not emit after the newer preparation"),
            )
            .is_none());
        assert!(matches!(engine.readiness(), SttReadiness::Unloaded));
    }

    #[test]
    fn failure_event_and_status_share_a_fence_and_old_success_cannot_clear_them() {
        let engine = SttEngine::new();
        let old_attempt = engine.begin_preparation();
        let current_attempt = engine.begin_preparation();
        let published = std::cell::Cell::new(false);
        let readiness = engine
            .report_remote_startup_failure(
                current_attempt,
                &AppError::Config("PRIVATE_CURRENT_FAILURE".into()),
                |readiness| {
                    assert!(engine.preparation.try_lock().is_none());
                    assert!(matches!(readiness, SttReadiness::Failed { .. }));
                    published.set(true);
                },
            )
            .unwrap();
        assert!(published.get());
        engine.finish_preparation(old_attempt, true);
        assert_eq!(
            serde_json::to_value(engine.readiness()).unwrap(),
            serde_json::to_value(readiness).unwrap()
        );
    }

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
