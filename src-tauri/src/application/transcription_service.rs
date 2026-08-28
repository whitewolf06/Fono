//! Tauri-independent application contract for local file transcription.
//!
//! HTTP, desktop UI and future adapters call this use case instead of owning a
//! Whisper instance. Audio decoding and job scheduling deliberately live in
//! later stories; this module accepts already-normalized PCM samples.

use std::path::PathBuf;
use std::sync::Arc;

use crate::application::transcription_contract::{
    validate_request, TranscriptionRequest, TranscriptionResult, TranscriptionServiceError,
    TranscriptionServiceResult,
};
use crate::error::AppResult;
use crate::stt::{SttEngine, WorkerPaths};
use crate::types::{AccelerationMode, Transcript};
use fono_core::OperationCancellation;

/// The only dependency the use case needs from the STT infrastructure.
/// No Tauri, CPAL, filesystem or window APIs cross this boundary.
pub trait TranscriptionRuntime: Send + Sync {
    fn ensure_ready(&self) -> AppResult<()>;
    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> AppResult<Transcript>;
}

/// Adapter for the existing shared engine. It configures the same runtime used
/// by desktop dictation; it never creates an additional Whisper instance.
pub struct SharedSttRuntime {
    engine: Arc<SttEngine>,
    model_path: PathBuf,
    acceleration: AccelerationMode,
    worker_paths: WorkerPaths,
}

impl SharedSttRuntime {
    pub fn new(
        engine: Arc<SttEngine>,
        model_path: PathBuf,
        acceleration: AccelerationMode,
        worker_paths: WorkerPaths,
    ) -> Self {
        Self {
            engine,
            model_path,
            acceleration,
            worker_paths,
        }
    }
}

impl TranscriptionRuntime for SharedSttRuntime {
    fn ensure_ready(&self) -> AppResult<()> {
        self.engine
            .ensure_loaded(&self.model_path, self.acceleration, &self.worker_paths)
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
        self.engine
            .transcribe_cancellable(pcm_samples, language, cancellation)
    }
}

pub struct TranscriptionService<R> {
    runtime: R,
}

impl<R> TranscriptionService<R>
where
    R: TranscriptionRuntime,
{
    pub fn new(runtime: R) -> Self {
        Self { runtime }
    }

    #[cfg(test)]
    pub(crate) fn runtime(&self) -> &R {
        &self.runtime
    }

    pub fn transcribe(
        &self,
        request: TranscriptionRequest,
    ) -> TranscriptionServiceResult<TranscriptionResult> {
        self.transcribe_cancellable(request, OperationCancellation::default())
    }

    pub fn transcribe_cancellable(
        &self,
        request: TranscriptionRequest,
        cancellation: OperationCancellation,
    ) -> TranscriptionServiceResult<TranscriptionResult> {
        validate_request(&request)?;
        if cancellation.is_cancelled() {
            return Err(TranscriptionServiceError::Cancelled(
                "transcription cancelled before readiness check".into(),
            ));
        }
        self.runtime
            .ensure_ready()
            .map_err(TranscriptionServiceError::from)?;
        if cancellation.is_cancelled() {
            return Err(TranscriptionServiceError::Cancelled(
                "transcription cancelled before inference".into(),
            ));
        }
        let transcript = self
            .runtime
            .transcribe(&request.pcm_samples, &request.language, cancellation)
            .map_err(TranscriptionServiceError::from)?;
        Ok(TranscriptionResult::from_transcript(
            transcript,
            request.model,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::application::transcription_contract::TRANSCRIPTION_PROTOCOL_VERSION;
    use crate::error::AppError;

    struct FakeRuntime {
        ensure_calls: AtomicUsize,
        transcript: Transcript,
        model_not_loaded: bool,
    }

    impl FakeRuntime {
        fn ready(transcript: Transcript) -> Self {
            Self {
                ensure_calls: AtomicUsize::new(0),
                transcript,
                model_not_loaded: false,
            }
        }
    }

    impl TranscriptionRuntime for FakeRuntime {
        fn ensure_ready(&self) -> AppResult<()> {
            self.ensure_calls.fetch_add(1, Ordering::Relaxed);
            if self.model_not_loaded {
                return Err(AppError::ModelNotLoaded);
            }
            Ok(())
        }

        fn transcribe(
            &self,
            _pcm_samples: &[i16],
            _language: &str,
            cancellation: OperationCancellation,
        ) -> AppResult<Transcript> {
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled("cancelled by test".into()));
            }
            Ok(self.transcript.clone())
        }
    }

    fn request() -> TranscriptionRequest {
        TranscriptionRequest {
            pcm_samples: vec![0, 4, -4],
            language: "ru".into(),
            model: "large-v3-turbo".into(),
        }
    }

    fn transcript() -> Transcript {
        Transcript {
            text: "проверка контракта".into(),
            detected_language: Some("ru".into()),
            audio_secs: Some(0.5),
            transcribe_secs: Some(0.1),
            device: Some("CUDA".into()),
        }
    }

    #[test]
    fn returns_versioned_result_from_the_shared_runtime_contract() {
        let service = TranscriptionService::new(FakeRuntime::ready(transcript()));

        let result = service.transcribe(request()).unwrap();

        assert_eq!(result.protocol_version, TRANSCRIPTION_PROTOCOL_VERSION);
        assert_eq!(result.text, "проверка контракта");
        assert_eq!(result.model, "large-v3-turbo");
        assert_eq!(result.backend.as_deref(), Some("CUDA"));
    }

    #[test]
    fn rejects_invalid_input_before_touching_the_runtime() {
        let runtime = FakeRuntime::ready(transcript());
        let service = TranscriptionService::new(runtime);
        let mut invalid = request();
        invalid.pcm_samples.clear();

        assert!(matches!(
            service.transcribe(invalid),
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
        assert_eq!(service.runtime.ensure_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cancellation_prevents_model_loading() {
        let runtime = FakeRuntime::ready(transcript());
        let service = TranscriptionService::new(runtime);
        let coordinator = fono_core::OperationCoordinator::new();
        let operation = coordinator
            .start(fono_core::OperationSource::Service)
            .unwrap();
        let cancellation = coordinator.cancellation(operation.id).unwrap();
        coordinator.cancel(operation.id).unwrap();

        assert!(matches!(
            service.transcribe_cancellable(request(), cancellation),
            Err(TranscriptionServiceError::Cancelled(_))
        ));
        assert_eq!(service.runtime.ensure_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn model_not_loaded_is_exposed_as_a_stable_error_code() {
        let runtime = FakeRuntime {
            ensure_calls: AtomicUsize::new(0),
            transcript: transcript(),
            model_not_loaded: true,
        };
        let service = TranscriptionService::new(runtime);

        assert!(matches!(
            service.transcribe(request()),
            Err(TranscriptionServiceError::ModelNotReady(_))
        ));
    }
}
