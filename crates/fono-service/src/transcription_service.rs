//! Tauri-independent application contract for local file transcription.
//!
//! HTTP, desktop UI and future adapters call this use case instead of owning a
//! Whisper instance. Audio decoding and job scheduling deliberately live in
//! later stories; this module accepts already-normalized PCM samples.

use std::path::PathBuf;
use std::sync::Arc;

use fono_core::OperationCancellation;
use fono_stt::{error::SttError, SttEngine, WorkerPaths};
use fono_types::{
    validate_request, AccelerationMode, CoreError, ErrorCode, SubtitleSegment, Transcript,
    TranscriptionRequest, TranscriptionResult, TranscriptionServiceError,
    TranscriptionServiceResult, WhisperArtifact,
};

/// The only dependency the use case needs from the STT infrastructure.
/// No Tauri, CPAL, filesystem or window APIs cross this boundary.
pub trait TranscriptionRuntime: Send + Sync {
    type Error: Into<TranscriptionServiceError>;

    /// Identifier of the model this runtime will actually use. It is public
    /// metadata, never a filesystem path.
    fn configured_model(&self) -> Result<String, Self::Error>;

    /// Hosts with a model pool may override selection per request. The
    /// desktop keeps its historical selected-model restriction by default.
    fn selected_model(&self, requested: Option<&str>) -> TranscriptionServiceResult<String> {
        let configured = self.configured_model().map_err(Into::into)?;
        if requested.is_some_and(|requested| requested != configured) {
            return Err(TranscriptionServiceError::InvalidRequest(format!(
                "model must match the selected Fono model ({configured})"
            )));
        }
        Ok(configured)
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> Result<Transcript, Self::Error>;

    fn transcribe_with_progress(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        _progress: &(dyn Fn(f32) + Sync),
    ) -> Result<(Transcript, Vec<SubtitleSegment>), Self::Error> {
        self.transcribe(pcm_samples, language, cancellation)
            .map(|transcript| (transcript, Vec::new()))
    }

    fn transcribe_selected_with_progress(
        &self,
        _model: &str,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> Result<(Transcript, Vec<SubtitleSegment>), Self::Error> {
        self.transcribe_with_progress(pcm_samples, language, cancellation, progress)
    }
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
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        configured_model_identifier(&self.model_path)
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        self.engine
            .ensure_loaded(&self.model_path, self.acceleration, &self.worker_paths)
            .map_err(service_error_from_stt)?;
        self.engine
            .transcribe_cancellable(pcm_samples, language, cancellation)
            .map_err(service_error_from_stt)
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
        self.transcribe_with_progress(request, cancellation, &|_| {})
    }

    pub fn transcribe_with_progress(
        &self,
        request: TranscriptionRequest,
        cancellation: OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> TranscriptionServiceResult<TranscriptionResult> {
        validate_request(&request)?;
        if cancellation.is_cancelled() {
            return Err(TranscriptionServiceError::Cancelled(
                "transcription cancelled before readiness check".into(),
            ));
        }
        let configured_model = self.runtime.selected_model(request.model.as_deref())?;
        let (transcript, segments) = self
            .runtime
            .transcribe_selected_with_progress(
                &configured_model,
                &request.pcm_samples,
                &request.language,
                cancellation,
                progress,
            )
            .map_err(Into::into)?;
        let mut result = TranscriptionResult::from_transcript(transcript, configured_model);
        result.segments = segments;
        Ok(result)
    }
}

pub fn configured_model_identifier(
    model_path: &std::path::Path,
) -> TranscriptionServiceResult<String> {
    let filename = model_path
        .file_name()
        .and_then(|filename| filename.to_str())
        .ok_or_else(|| TranscriptionServiceError::ModelNotReady("model is not loaded".into()))?;
    WhisperArtifact::from_filename(filename)
        .map(|model| model.variant)
        .ok_or_else(|| TranscriptionServiceError::ModelNotReady("model is not loaded".into()))
}

fn service_error_from_stt(error: SttError) -> TranscriptionServiceError {
    let public = CoreError::from(&error);
    match public.code {
        ErrorCode::Busy => TranscriptionServiceError::Busy(public.message),
        ErrorCode::Cancelled => TranscriptionServiceError::Cancelled(public.message),
        ErrorCode::ModelNotReady | ErrorCode::ModelNotFound => {
            TranscriptionServiceError::ModelNotReady(public.message)
        }
        _ => TranscriptionServiceError::Failed(public.message),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use fono_types::TRANSCRIPTION_PROTOCOL_VERSION;

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
        type Error = TranscriptionServiceError;

        fn configured_model(&self) -> Result<String, Self::Error> {
            if self.model_not_loaded {
                return Err(TranscriptionServiceError::ModelNotReady(
                    "model is not loaded".into(),
                ));
            }
            Ok("large_turbo".into())
        }

        fn transcribe(
            &self,
            _pcm_samples: &[i16],
            language: &str,
            cancellation: OperationCancellation,
        ) -> Result<Transcript, Self::Error> {
            self.transcribe_calls.fetch_add(1, Ordering::Relaxed);
            *self.requested_language.lock().unwrap() = Some(language.into());
            if cancellation.is_cancelled() {
                return Err(TranscriptionServiceError::Cancelled(
                    "cancelled by test".into(),
                ));
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
            remote: None,
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
    fn pooled_runtime_routes_requested_model_and_reports_the_actual_selection() {
        struct PooledRuntime {
            requested: Mutex<Vec<String>>,
        }
        impl TranscriptionRuntime for PooledRuntime {
            type Error = TranscriptionServiceError;

            fn configured_model(&self) -> Result<String, Self::Error> {
                Ok("whisper:base".into())
            }

            fn selected_model(
                &self,
                requested: Option<&str>,
            ) -> TranscriptionServiceResult<String> {
                let model = requested.unwrap_or("whisper:base");
                match model {
                    "whisper:base" | "whisper:tiny" => Ok(model.into()),
                    _ => Err(TranscriptionServiceError::ModelNotFound(
                        "unknown model".into(),
                    )),
                }
            }

            fn transcribe(
                &self,
                _pcm_samples: &[i16],
                _language: &str,
                _cancellation: OperationCancellation,
            ) -> Result<Transcript, Self::Error> {
                panic!("pooled requests must use the explicitly selected model")
            }

            fn transcribe_selected_with_progress(
                &self,
                model: &str,
                pcm_samples: &[i16],
                language: &str,
                _cancellation: OperationCancellation,
                progress: &(dyn Fn(f32) + Sync),
            ) -> Result<(Transcript, Vec<SubtitleSegment>), Self::Error> {
                assert_eq!(pcm_samples, &[0, 4, -4]);
                assert_eq!(language, "ru");
                self.requested.lock().unwrap().push(model.into());
                progress(1.0);
                Ok((transcript(), Vec::new()))
            }
        }

        let service = TranscriptionService::new(PooledRuntime {
            requested: Mutex::new(Vec::new()),
        });
        let selected = service
            .transcribe(TranscriptionRequest {
                model: Some("whisper:tiny".into()),
                ..request()
            })
            .unwrap();
        assert_eq!(selected.model, "whisper:tiny");
        let default = service
            .transcribe(TranscriptionRequest {
                model: None,
                ..request()
            })
            .unwrap();
        assert_eq!(default.model, "whisper:base");
        assert!(matches!(
            service.transcribe(TranscriptionRequest {
                model: Some("unknown".into()),
                ..request()
            }),
            Err(TranscriptionServiceError::ModelNotFound(_))
        ));
        assert_eq!(
            &*service.runtime.requested.lock().unwrap(),
            &["whisper:tiny", "whisper:base"]
        );
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
