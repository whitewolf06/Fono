use base64::Engine;
use fono_stt_protocol::{
    BackendKind, RequestMeta, WindowTranscript, WorkerRequest, WorkerResponse,
    MAX_RESPONSE_FRAME_BYTES, PROTOCOL_VERSION,
};
use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub struct Worker {
    child: Child,
    input: ChildStdin,
    responses: Receiver<Result<WorkerResponse, String>>,
    model: String,
    sequence: u64,
    pub backend: BackendKind,
}

impl Worker {
    pub fn load(path: &str, model: &str) -> Result<Self, String> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| error.to_string())?;
        let input = child.stdin.take().ok_or("worker stdin unavailable")?;
        let output = child.stdout.take().ok_or("worker stdout unavailable")?;
        let (sender, responses) = mpsc::sync_channel(2);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let response =
                    crate::framing::read_limited_line(&mut reader, MAX_RESPONSE_FRAME_BYTES)
                        .map_err(|error| error.to_string())
                        .and_then(|line| line.ok_or("worker EOF".into()))
                        .and_then(|line| {
                            serde_json::from_str(&line).map_err(|error| error.to_string())
                        });
                let terminal = response.is_err();
                if sender.send(response).is_err() || terminal {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            input,
            responses,
            model: model.into(),
            sequence: 0,
            backend: BackendKind::Cpu,
        };
        let meta = worker.meta();
        let hello = worker.request(WorkerRequest::Hello { meta })?;
        if let WorkerResponse::Ready {
            backend,
            capabilities,
            ..
        } = hello
        {
            if !capabilities.supports_window || !capabilities.supports_cancel {
                return Err("worker lacks v3 capabilities".into());
            }
            worker.backend = backend;
        } else {
            return Err("worker did not return Ready".into());
        }
        let meta = worker.meta();
        if !matches!(
            worker.request(WorkerRequest::Load {
                meta,
                model_path: model.into()
            })?,
            WorkerResponse::ModelLoaded { .. }
        ) {
            return Err("worker did not load model".into());
        }
        Ok(worker)
    }
    fn meta(&mut self) -> RequestMeta {
        self.sequence += 1;
        RequestMeta::new(format!("bench-{}", self.sequence), Some("benchmark".into()))
    }
    fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse, String> {
        let id = request.meta().request_id.clone();
        writeln!(
            self.input,
            "{}",
            serde_json::to_string(&request).map_err(|error| error.to_string())?
        )
        .and_then(|_| self.input.flush())
        .map_err(|error| error.to_string())?;
        let response = self
            .responses
            .recv_timeout(Duration::from_secs(600))
            .map_err(|error| error.to_string())??;
        if response.protocol_version() != PROTOCOL_VERSION || response.request_id() != id {
            return Err("worker envelope mismatch".into());
        }
        if let WorkerResponse::Error { code, message, .. } = &response {
            return Err(format!("{code}: {message}"));
        }
        Ok(response)
    }
    pub fn window(
        &mut self,
        samples: &[i16],
        language: &str,
        start: u64,
        context: Option<&str>,
    ) -> Result<WindowTranscript, String> {
        let bytes: Vec<_> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let meta = self.meta();
        match self.request(WorkerRequest::TranscribeWindow {
            meta,
            model_path: self.model.clone(),
            language: language.into(),
            context: context.map(str::to_owned),
            audio_start_sample: start,
            samples_i16_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        })? {
            WorkerResponse::WindowResult { transcript, .. } => Ok(transcript),
            _ => Err("worker did not return a window result".into()),
        }
    }
    pub fn batch(&mut self, samples: &[i16], language: &str) -> Result<WindowTranscript, String> {
        let bytes: Vec<_> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let meta = self.meta();
        match self.request(WorkerRequest::Transcribe {
            meta,
            model_path: self.model.clone(),
            language: language.into(),
            samples_i16_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        })? {
            WorkerResponse::Result {
                text,
                audio_secs,
                transcribe_secs,
                backend,
                ..
            } => Ok(WindowTranscript {
                text,
                segments: vec![],
                words: vec![],
                detected_language: None,
                audio_secs,
                transcribe_secs,
                backend,
            }),
            _ => Err("worker did not return a batch result".into()),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
