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
use crate::types::{AccelerationMode, Transcript, WhisperModelSize};
use fono_core::OperationCancellation;

/// The only dependency the use case needs from the STT infrastructure.
/// No Tauri, CPAL, filesystem or window APIs cross this boundary.
pub trait TranscriptionRuntime: Send + Sync {
    /// Identifier of the model this runtime will actually use. It is public
    /// metadata, never a filesystem path.
    fn configured_model(&self) -> AppResult<String>;

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
    fn configured_model(&self) -> AppResult<String> {
        configured_model_identifier(&self.model_path)
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
        self.engine
            .ensure_loaded(&self.model_path, self.acceleration, &self.worker_paths)?;
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
        let configured_model = self
            .runtime
            .configured_model()
            .map_err(TranscriptionServiceError::from)?;
        if let Some(requested_model) = request.model.as_deref() {
            if requested_model != configured_model {
                return Err(TranscriptionServiceError::InvalidRequest(format!(
                    "model must match the selected Fono model ({configured_model})"
                )));
            }
        }
        let transcript = self
            .runtime
            .transcribe(&request.pcm_samples, &request.language, cancellation)
            .map_err(TranscriptionServiceError::from)?;
        Ok(TranscriptionResult::from_transcript(
            transcript,
            configured_model,
        ))
    }
}

pub fn configured_model_identifier(model_path: &std::path::Path) -> AppResult<String> {
    let filename = model_path
        .file_name()
        .and_then(|filename| filename.to_str())
        .ok_or(crate::error::AppError::ModelNotLoaded)?;
    WhisperModelSize::from_filename(filename)
        .map(|model| model.api_identifier().to_string())
        .ok_or(crate::error::AppError::ModelNotLoaded)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::application::transcription_contract::TRANSCRIPTION_PROTOCOL_VERSION;
    use crate::error::AppError;

    struct FakeRuntime {
        transcribe_calls: AtomicUsize,
        requested_language: Mutex<Option<String>>,
        transcript: Transcript,
        model_not_loaded: bool,
    }

    impl FakeRuntime {
        fn ready(transcript: Transcript) -> Self {
            Self {
                transcribe_calls: AtomicUsize::new(0),
                requested_language: Mutex::new(None),
                transcript,
                model_not_loaded: false,
            }
        }
    }

    impl TranscriptionRuntime for FakeRuntime {
        fn configured_model(&self) -> AppResult<String> {
            if self.model_not_loaded {
                return Err(AppError::ModelNotLoaded);
            }
            Ok("large_turbo".into())
        }

        fn transcribe(
            &self,
            _pcm_samples: &[i16],
            language: &str,
            cancellation: OperationCancellation,
        ) -> AppResult<Transcript> {
            self.transcribe_calls.fetch_add(1, Ordering::Relaxed);
            *self.requested_language.lock().unwrap() = Some(language.into());
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
            model: Some("large_turbo".into()),
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
        assert_eq!(result.model, "large_turbo");
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
        assert_eq!(service.runtime.transcribe_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rejects_a_model_other_than_the_active_fono_model_before_inference() {
        let runtime = FakeRuntime::ready(transcript());
        let service = TranscriptionService::new(runtime);
        let mut mismatched = request();
        mismatched.model = Some("base".into());

        assert!(matches!(
            service.transcribe(mismatched),
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
        assert_eq!(service.runtime.transcribe_calls.load(Ordering::Relaxed), 0);
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
        assert_eq!(service.runtime.transcribe_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn model_not_loaded_is_exposed_as_a_stable_error_code() {
        let runtime = FakeRuntime {
            transcribe_calls: AtomicUsize::new(0),
            requested_language: Mutex::new(None),
            transcript: transcript(),
            model_not_loaded: true,
        };
        let service = TranscriptionService::new(runtime);

        assert!(matches!(
            service.transcribe(TranscriptionRequest {
                model: None,
                ..request()
            }),
            Err(TranscriptionServiceError::ModelNotReady(_))
        ));
    }

    #[test]
    fn omitted_model_uses_the_configured_model_and_preserves_language() {
        let runtime = FakeRuntime::ready(transcript());
        let service = TranscriptionService::new(runtime);
        let request = TranscriptionRequest {
            model: None,
            ..request()
        };

        let result = service.transcribe(request).unwrap();

        assert_eq!(result.model, "large_turbo");
        assert_eq!(
            service
                .runtime
                .requested_language
                .lock()
                .unwrap()
                .as_deref(),
            Some("ru")
        );
    }

    #[test]
    fn rejects_an_empty_explicit_model() {
        let service = TranscriptionService::new(FakeRuntime::ready(transcript()));
        let request = TranscriptionRequest {
            model: Some("   ".into()),
            ..request()
        };

        assert!(matches!(
            service.transcribe(request),
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
    }
}
