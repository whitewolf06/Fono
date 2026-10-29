//! Bounded OpenAI-style file transcription over the same recognition pipeline.
//!
//! Supported request fields and output formats follow the audio transcription
//! contract. Server model identifiers come from `/v1/models`.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::{Extension, FromRequest, Multipart, Request};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use fono_service::rest_api::{RestApiState, TokenPrincipal};
use fono_service::transcript_export::{self, ExportFormat};
use fono_stt::scheduler::QueueClass;
use serde_json::{json, Value};

use crate::recognition_options::{RecognitionOptions, TimestampMode};
use crate::recognize::{
    self, AudioFormat, CompletedRecognition, HttpError, RecognitionInput, RecognizeState, RequestId,
};

// A UTF-8 byte cap no larger than the native token budget conservatively
// guarantees that the prompt fits instead of silently losing its context.
const MAX_PROMPT_BYTES: usize = fono_stt_protocol::prompt::MAX_PROMPT_TOKENS;

pub(crate) fn routes() -> Router<RestApiState> {
    Router::new().route("/v1/audio/transcriptions", post(transcribe))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ResponseFormat {
    #[default]
    Json,
    Text,
    Srt,
    Vtt,
    VerboseJson,
}

#[derive(Default)]
struct Fields {
    audio: Option<(Vec<u8>, AudioFormat)>,
    model: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    format: ResponseFormat,
    granularities: HashSet<String>,
}

impl Fields {
    fn into_input(self) -> Result<(RecognitionInput, ResponseFormat, HashSet<String>), HttpError> {
        let (body, format) = self.audio.ok_or_else(HttpError::invalid)?;
        let model = self
            .model
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(HttpError::invalid)?;
        let language = self.language.unwrap_or_else(|| "auto".into());
        if language != "auto"
            && !(language.len() == 2 && language.bytes().all(|byte| byte.is_ascii_lowercase()))
        {
            return Err(HttpError::invalid());
        }
        if !self.granularities.is_empty() && self.format != ResponseFormat::VerboseJson {
            return Err(HttpError::invalid());
        }
        let mut granularities = self.granularities;
        if self.format == ResponseFormat::VerboseJson && granularities.is_empty() {
            granularities.insert("segment".into());
        }
        let timestamps = if granularities.contains("word") {
            TimestampMode::Words
        } else if granularities.contains("segment")
            || matches!(self.format, ResponseFormat::Srt | ResponseFormat::Vtt)
        {
            TimestampMode::Segments
        } else {
            TimestampMode::None
        };
        Ok((
            RecognitionInput {
                body,
                format,
                language,
                model: Some(model),
                class: QueueClass::Batch,
                options: RecognitionOptions {
                    prompt: self.prompt.filter(|value| !value.is_empty()),
                    timestamps,
                    ..RecognitionOptions::default()
                },
            },
            self.format,
            granularities,
        ))
    }
}

async fn transcribe(
    Extension(state): Extension<Arc<RecognizeState>>,
    Extension(principal): Extension<TokenPrincipal>,
    Extension(request_id): Extension<RequestId>,
    request: Request,
) -> Response {
    let admission = match recognize::admit(&state, principal.client_key).await {
        Ok(admission) => admission,
        Err(error) => return error.into_openai_response(&request_id),
    };
    let max_file_bytes = state.policy.max_file_bytes;
    let read_body = async {
        let multipart = Multipart::from_request(request, &())
            .await
            .map_err(|error| multipart_status(error.status()))?;
        read_fields(multipart, max_file_bytes).await?.into_input()
    };
    let (input, format, granularities) =
        match tokio::time::timeout(state.body_read_timeout, read_body).await {
            Ok(Ok(input)) => input,
            Ok(Err(error)) => return error.into_openai_response(&request_id),
            Err(_) => return HttpError::body_timeout().into_openai_response(&request_id),
        };
    match recognize::execute(state, admission, input).await {
        Ok(completed) => render(completed, format, &granularities)
            .unwrap_or_else(|error| error.into_openai_response(&request_id)),
        Err(error) => error.into_openai_response(&request_id),
    }
}

async fn read_fields(mut multipart: Multipart, max_file_bytes: u64) -> Result<Fields, HttpError> {
    let mut parsed = Fields::default();
    let mut seen = HashSet::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| multipart_status(error.status()))?
    {
        let name = field.name().ok_or_else(HttpError::invalid)?.to_owned();
        if name != "timestamp_granularities[]"
            && name != "timestamp_granularities"
            && !seen.insert(name.clone())
        {
            return Err(HttpError::invalid());
        }
        if name == "file" {
            let format = file_format(field.file_name().ok_or_else(HttpError::unsupported)?)?;
            let mut body = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|error| multipart_status(error.status()))?
            {
                if chunk.len() as u64 > max_file_bytes.saturating_sub(body.len() as u64) {
                    return Err(HttpError::too_large());
                }
                body.extend_from_slice(&chunk);
            }
            if body.is_empty() {
                return Err(HttpError::invalid());
            }
            parsed.audio = Some((body, format));
            continue;
        }
        let limit = match name.as_str() {
            "prompt" => MAX_PROMPT_BYTES,
            "model" => 128,
            "language" => 16,
            "response_format"
            | "timestamp_granularities[]"
            | "timestamp_granularities"
            | "temperature"
            | "stream" => 32,
            _ => return Err(HttpError::invalid()),
        };
        let value = read_text(field, limit).await?;
        match name.as_str() {
            "model" => parsed.model = Some(value),
            "language" => parsed.language = Some(value),
            "prompt" => parsed.prompt = Some(value),
            "response_format" => {
                parsed.format = match value.as_str() {
                    "json" => ResponseFormat::Json,
                    "text" => ResponseFormat::Text,
                    "srt" => ResponseFormat::Srt,
                    "vtt" => ResponseFormat::Vtt,
                    "verbose_json" => ResponseFormat::VerboseJson,
                    _ => return Err(HttpError::invalid()),
                }
            }
            "timestamp_granularities[]" | "timestamp_granularities" => {
                if !matches!(value.as_str(), "word" | "segment")
                    || !parsed.granularities.insert(value)
                {
                    return Err(HttpError::invalid());
                }
            }
            "temperature" => {
                let temperature: f64 = value.parse().map_err(|_| HttpError::invalid())?;
                // The server uses the configured recognizer's deterministic
                // decoding. Temperature is accepted for SDK compatibility.
                if !temperature.is_finite() || !(0.0..=1.0).contains(&temperature) {
                    return Err(HttpError::invalid());
                }
            }
            "stream" if value == "false" => {}
            _ => return Err(HttpError::invalid()),
        }
    }
    Ok(parsed)
}

async fn read_text(
    mut field: axum::extract::multipart::Field<'_>,
    limit: usize,
) -> Result<String, HttpError> {
    let mut body = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| multipart_status(error.status()))?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(HttpError::invalid());
        }
        body.extend_from_slice(&chunk);
    }
    let value = String::from_utf8(body).map_err(|_| HttpError::invalid())?;
    if value.contains('\0') {
        return Err(HttpError::invalid());
    }
    Ok(value)
}

fn multipart_status(status: StatusCode) -> HttpError {
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        HttpError::too_large()
    } else if status.is_server_error() {
        HttpError::internal()
    } else {
        HttpError::invalid()
    }
}

fn file_format(filename: &str) -> Result<AudioFormat, HttpError> {
    let extension = filename
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "wav" => Ok(AudioFormat::Wav),
        "mp3" => Ok(AudioFormat::Mp3),
        "flac" => Ok(AudioFormat::Flac),
        "ogg" | "opus" => Ok(AudioFormat::Ogg),
        _ => Err(HttpError::unsupported()),
    }
}

fn render(
    completed: CompletedRecognition,
    format: ResponseFormat,
    granularities: &HashSet<String>,
) -> Result<Response, HttpError> {
    let result = completed.output.result;
    Ok(match format {
        ResponseFormat::Json => Json(json!({"text": result.text})).into_response(),
        ResponseFormat::Text => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            result.text,
        )
            .into_response(),
        ResponseFormat::Srt | ResponseFormat::Vtt => {
            let export = if format == ResponseFormat::Srt {
                ExportFormat::Srt
            } else {
                ExportFormat::Vtt
            };
            let text = if result.text.trim().is_empty() {
                if format == ResponseFormat::Vtt {
                    "WEBVTT\n\n".into()
                } else {
                    String::new()
                }
            } else {
                transcript_export::render(&result, export).map_err(|_| HttpError::invalid())?
            };
            let content_type = if format == ResponseFormat::Srt {
                "application/x-subrip; charset=utf-8"
            } else {
                "text/vtt; charset=utf-8"
            };
            ([(header::CONTENT_TYPE, content_type)], text).into_response()
        }
        ResponseFormat::VerboseJson => {
            let mut json = json!({
                "task": "transcribe",
                "language": result.detected_language.or_else(|| (completed.requested_language != "auto").then_some(completed.requested_language)),
                "duration": completed.audio_ms as f64 / 1000.0,
                "text": result.text,
            });
            if granularities.contains("segment") {
                json["segments"] = Value::Array(
                    result
                        .segments
                        .iter()
                        .enumerate()
                        .map(|(id, segment)| {
                            json!({
                                "id": id, "start": segment.start_ms as f64 / 1000.0,
                                "end": segment.end_ms as f64 / 1000.0, "text": segment.text,
                            })
                        })
                        .collect(),
                );
            }
            if granularities.contains("word") {
                json["words"] = Value::Array(
                    completed
                        .output
                        .words
                        .iter()
                        .map(|word| {
                            json!({
                                "word": word.text, "start": word.start_ms as f64 / 1000.0,
                                "end": word.end_ms as f64 / 1000.0,
                            })
                        })
                        .collect(),
                );
            }
            Json(json).into_response()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::ServerActivity;
    use crate::recognition_options::RecognitionOutput;
    use crate::recognize::{AdmittedRecognition, InteractiveRecognizer};
    use axum::body::{to_bytes, Body};
    use axum::extract::DefaultBodyLimit;
    use fono_core::OperationCancellation;
    use fono_service::audio_ingest::AudioIngestPolicy;
    use fono_service::rest_api::TokenScope;
    use fono_types::{SubtitleSegment, TranscriptionResult};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    use std::time::Duration;
    use tower::ServiceExt;

    fn completed() -> CompletedRecognition {
        CompletedRecognition {
            output: RecognitionOutput {
                result: TranscriptionResult {
                    protocol_version: 1,
                    text: "Hello world".into(),
                    detected_language: Some("en".into()),
                    audio_seconds: Some(2.0),
                    transcribe_seconds: Some(0.1),
                    model: "whisper:tiny".into(),
                    backend: Some("cpu".into()),
                    segments: vec![SubtitleSegment {
                        start_ms: 250,
                        end_ms: 1750,
                        text: "Hello world".into(),
                    }],
                },
                words: vec![SubtitleSegment {
                    start_ms: 250,
                    end_ms: 700,
                    text: "Hello".into(),
                }],
            },
            audio_ms: 2000,
            requested_language: "auto".into(),
        }
    }

    #[tokio::test]
    async fn sdk_formats_use_real_timings_and_do_not_fabricate_statistics() {
        for (format, expected) in [
            (ResponseFormat::Text, "Hello world"),
            (ResponseFormat::Srt, "00:00:00,250 --> 00:00:01,750"),
            (ResponseFormat::Vtt, "00:00:00.250 --> 00:00:01.750"),
        ] {
            let response = render(completed(), format, &HashSet::new()).unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            assert!(std::str::from_utf8(&body).unwrap().contains(expected));
        }
        let response = render(
            completed(),
            ResponseFormat::VerboseJson,
            &HashSet::from(["segment".into(), "word".into()]),
        )
        .unwrap();
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["duration"], 2.0);
        assert_eq!(value["segments"][0]["start"], 0.25);
        assert_eq!(value["words"][0]["word"], "Hello");
        assert!(value["segments"][0].get("avg_logprob").is_none());
    }

    fn multipart_request(parts: &[(&str, &str)], file: &[u8]) -> Request {
        let mut body = Vec::new();
        for (name, value) in parts {
            body.extend_from_slice(
                format!(
                    "--test\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .as_bytes(),
            );
        }
        body.extend_from_slice(b"--test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"sample.wav\"\r\nContent-Type: audio/wav\r\n\r\n");
        body.extend_from_slice(file);
        body.extend_from_slice(b"\r\n--test--\r\n");
        Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=test")
            .body(Body::from(body))
            .unwrap()
    }

    async fn fields(
        parts: &[(&str, &str)],
        file_bytes: usize,
        limit: u64,
    ) -> Result<Fields, HttpError> {
        let request = multipart_request(parts, &vec![1; file_bytes]);
        let multipart = Multipart::from_request(request, &()).await.unwrap();
        read_fields(multipart, limit).await
    }

    type CapturedOptions = Arc<Mutex<Option<(QueueClass, RecognitionOptions)>>>;

    struct CaptureRecognizer {
        held: Arc<AtomicUsize>,
        options: CapturedOptions,
    }

    impl InteractiveRecognizer for CaptureRecognizer {
        fn admit(
            self: Arc<Self>,
            _client_key: &str,
        ) -> Result<AdmittedRecognition, fono_types::TranscriptionServiceError> {
            struct Lease(Arc<AtomicUsize>);
            impl Drop for Lease {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::AcqRel);
                }
            }
            self.held.fetch_add(1, Ordering::AcqRel);
            let lease = Lease(Arc::clone(&self.held));
            Ok(Box::new(
                move |request, class, options, _cancellation: OperationCancellation| {
                    let _lease = lease;
                    assert!(!request.pcm_samples.is_empty());
                    *self.options.lock().unwrap() = Some((class, options));
                    Ok(completed().output)
                },
            ))
        }
    }

    fn state(timeout: Duration) -> (Arc<RecognizeState>, Arc<AtomicUsize>, CapturedOptions) {
        let held = Arc::new(AtomicUsize::new(0));
        let options = Arc::new(Mutex::new(None));
        let recognizer = Arc::new(CaptureRecognizer {
            held: Arc::clone(&held),
            options: Arc::clone(&options),
        });
        (
            RecognizeState::new(
                recognizer,
                ServerActivity::new(),
                std::env::temp_dir(),
                AudioIngestPolicy {
                    max_file_bytes: 8192,
                    max_duration_seconds: 1,
                },
                timeout,
            ),
            held,
            options,
        )
    }

    #[tokio::test]
    async fn openai_handler_decodes_public_wav_and_passes_batch_options() {
        let (state, held, options) = state(Duration::from_secs(1));
        let wav = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/fono-service/tests/fixtures/audio/tone-16k-mono.wav"
        ));
        let response = transcribe(
            Extension(state),
            Extension(TokenPrincipal {
                client_key: "test".into(),
                scope: TokenScope::Recognize,
            }),
            Extension(RequestId("openai-test".into())),
            multipart_request(
                &[("model", "whisper:tiny"), ("prompt", "Prior context")],
                wav,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["text"],
            "Hello world"
        );
        assert_eq!(held.load(Ordering::Acquire), 0);
        let options = options.lock().unwrap();
        let (class, options) = options.as_ref().unwrap();
        assert_eq!(*class, QueueClass::Batch);
        assert_eq!(options.prompt.as_deref(), Some("Prior context"));
    }

    #[tokio::test]
    async fn stalled_openai_multipart_obeys_body_deadline_and_releases_admission() {
        let (state, held, options) = state(Duration::from_millis(30));
        let request = Request::builder()
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=test")
            .body(Body::from_stream(futures_util::stream::pending::<
                Result<axum::body::Bytes, std::io::Error>,
            >()))
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            transcribe(
                Extension(state),
                Extension(TokenPrincipal {
                    client_key: "test".into(),
                    scope: TokenScope::Recognize,
                }),
                Extension(RequestId("openai-timeout".into())),
                request,
            ),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let error: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["error"]["code"], "body_read_timeout");
        assert_eq!(error["error"]["type"], "invalid_request_error");
        assert_eq!(held.load(Ordering::Acquire), 0);
        assert!(options.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn multipart_envelope_limit_returns_413_and_releases_admission() {
        // The four-byte file is below the per-file policy; the multipart
        // envelope exceeds the HTTP body layer. Check both one-frame and
        // fragmented delivery so parser/field errors preserve the limit code.
        for fragmented in [false, true] {
            let (state, held, options) = state(Duration::from_secs(1));
            let app = Router::new()
                .route("/", post(transcribe))
                .layer(DefaultBodyLimit::max(128))
                .layer(Extension(state))
                .layer(Extension(TokenPrincipal {
                    client_key: "test".into(),
                    scope: TokenScope::Recognize,
                }))
                .layer(Extension(RequestId("openai-envelope".into())));
            let mut request = multipart_request(&[("model", "whisper:tiny")], &[1, 2, 3, 4]);
            if fragmented {
                let (parts, body) = request.into_parts();
                let bytes = to_bytes(body, 4096).await.unwrap();
                request = Request::from_parts(
                    parts,
                    Body::from_stream(futures_util::stream::iter([
                        Ok::<_, std::io::Error>(bytes.slice(..64)),
                        Ok(bytes.slice(64..)),
                    ])),
                );
            }
            let response = app.oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["error"]["code"], "payload_too_large");
            assert_eq!(held.load(Ordering::Acquire), 0);
            assert!(options.lock().unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn multipart_accepts_sdk_fields_and_selects_batch_word_timing() {
        let fields = fields(
            &[
                ("model", "whisper:tiny"),
                ("response_format", "verbose_json"),
                ("timestamp_granularities[]", "word"),
                ("timestamp_granularities[]", "segment"),
                ("temperature", "0.5"),
                ("prompt", "Prior context"),
            ],
            4,
            100,
        )
        .await
        .unwrap();
        let (input, format, granularities) = fields.into_input().unwrap();
        assert_eq!(format, ResponseFormat::VerboseJson);
        assert_eq!(input.class, QueueClass::Batch);
        assert!(matches!(input.options.timestamps, TimestampMode::Words));
        assert_eq!(input.options.prompt.as_deref(), Some("Prior context"));
        assert_eq!(granularities.len(), 2);
    }

    #[tokio::test]
    async fn prompt_limit_counts_utf8_bytes_and_rejects_excess_before_inference() {
        let prompt = "я".repeat(MAX_PROMPT_BYTES / 2);
        assert_eq!(prompt.len(), MAX_PROMPT_BYTES);
        let parsed = fields(&[("model", "whisper:tiny"), ("prompt", &prompt)], 1, 100)
            .await
            .unwrap();
        let (input, _, _) = parsed.into_input().unwrap();
        assert_eq!(input.options.prompt.as_deref(), Some(prompt.as_str()));
        let oversized = format!("{prompt}я");
        assert!(oversized.chars().count() < MAX_PROMPT_BYTES);
        assert!(
            fields(&[("model", "whisper:tiny"), ("prompt", &oversized)], 1, 100)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn multipart_rejects_bad_fields_duplicates_and_oversized_upload_before_inference() {
        for parts in [
            vec![("model", "whisper:tiny"), ("model", "whisper:tiny")],
            vec![("temperature", "NaN")],
            vec![("stream", "true")],
            vec![("unknown", "value")],
        ] {
            assert!(fields(&parts, 4, 100).await.is_err());
        }
        assert!(fields(&[("model", "whisper:tiny")], 101, 100)
            .await
            .is_err());
        assert!(
            fields(&[("prompt", &"x".repeat(MAX_PROMPT_BYTES + 1))], 1, 100)
                .await
                .is_err()
        );
        assert!(fields(&[], 1, 100).await.unwrap().into_input().is_err());
        assert!(fields(
            &[
                ("model", "whisper:tiny"),
                ("timestamp_granularities[]", "word")
            ],
            1,
            100
        )
        .await
        .unwrap()
        .into_input()
        .is_err());
    }
}
