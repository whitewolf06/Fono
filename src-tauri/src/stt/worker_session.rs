impl WorkerSession {
    pub(super) fn start(path: PathBuf, backend: BackendKind, model_path: &Path) -> AppResult<Self> {
        Self::start_with_timeouts(path, backend, model_path, PRODUCTION_TIMEOUTS)
    }

    fn start_with_timeouts(
        path: PathBuf,
        backend: BackendKind,
        model_path: &Path,
        timeouts: WorkerTimeouts,
    ) -> AppResult<Self> {
        let mut command = Command::new(&path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // whisper.cpp emits progress and backend diagnostics to stderr.
            // Drain it on a dedicated thread and retain only a bounded tail.
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            // GPU workers are helper processes, not interactive terminals.
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|error| {
            AppError::Stt(format!(
                "cannot start {} worker: {error}",
                backend_name(backend)
            ))
        })?;
        let Some(stdin) = child.stdin.take() else {
            terminate_child(&mut child);
            return Err(AppError::Stt("worker stdin is unavailable".into()));
        };
        let Some(stdout) = child.stdout.take() else {
            terminate_child(&mut child);
            return Err(AppError::Stt("worker stdout is unavailable".into()));
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_child(&mut child);
            return Err(AppError::Stt("worker stderr is unavailable".into()));
        };
        let (stdout_rx, stdout_thread) = spawn_stdout_reader(stdout);
        let (stdin_tx, stdin_thread) = spawn_stdin_writer(stdin);
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        let stderr_thread = spawn_stderr_reader(stderr, stderr_tail.clone());
        let mut worker = Self {
            backend,
            path,
            model_path: model_path.to_string_lossy().to_string(),
            child,
            stdin_tx: Some(stdin_tx),
            stdin_thread: Some(stdin_thread),
            stdout_rx,
            stdout_thread: Some(stdout_thread),
            stderr_tail,
            stderr_thread: Some(stderr_thread),
            next_request_id: 0,
            timeouts,
        };
        let hello_meta = worker.next_meta("hello", None);
        match worker.request(&WorkerRequest::Hello { meta: hello_meta })? {
            WorkerResponse::Ready {
                backend: actual,
                capabilities,
                ..
            } if actual == backend
                && capabilities.protocol_version == PROTOCOL_VERSION
                && capabilities.supports_window
                && capabilities.supports_cancel
                && capabilities.supports_token_timestamps
                && capabilities.maximum_request_bytes >= MAX_REQUEST_FRAME_BYTES
                && capabilities.maximum_response_bytes >= MAX_RESPONSE_FRAME_BYTES => {}
            other => {
                return Err(AppError::Stt(format!(
                    "{} worker returned an unexpected handshake: {other:?}",
                    backend_name(backend)
                )))
            }
        }
        let load_meta = worker.next_meta("load", None);
        match worker.request(&WorkerRequest::Load {
            meta: load_meta,
            model_path: worker.model_path.clone(),
        })? {
            WorkerResponse::ModelLoaded {
                backend: actual, ..
            } if actual == backend => {}
            WorkerResponse::Error { message, .. } => {
                return Err(AppError::Stt(format!(
                    "{} worker could not load model: {message}",
                    backend_name(backend)
                )))
            }
            other => {
                return Err(AppError::Stt(format!(
                    "{} worker returned an unexpected load result: {other:?}",
                    backend_name(backend)
                )))
            }
        }
        tracing::info!("{} worker loaded the Whisper model", backend_name(backend));
        Ok(worker)
    }

    pub(super) fn device(&self) -> &'static str {
        backend_name(self.backend)
    }

    pub(super) fn ping(&mut self) -> AppResult<()> {
        let meta = self.next_meta("ping", None);
        match self.request(&WorkerRequest::Ping { meta: meta.clone() })? {
            WorkerResponse::Pong {
                backend,
                request_id,
                ..
            } if backend == self.backend && request_id == meta.request_id => Ok(()),
            other => Err(AppError::Stt(format!(
                "{} worker returned an unexpected health response: {other:?}",
                backend_name(self.backend)
            ))),
        }
    }

    #[cfg(test)]
    pub(super) fn transcribe(&mut self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_with_cancellation(samples, language, None)
    }

    pub(super) fn transcribe_cancellable(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe_with_cancellation(samples, language, Some(cancellation))
    }

    fn transcribe_with_cancellation(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: Option<&OperationCancellation>,
    ) -> AppResult<Transcript> {
        let meta = self.next_meta("transcribe", None);
        let request_id = meta.request_id.clone();
        let operation_id = format!("dictation-{request_id}");
        let response = self.request_with_cancellation(
            &WorkerRequest::Transcribe {
                meta: RequestMeta {
                    operation_id: Some(operation_id.clone()),
                    ..meta
                },
                model_path: self.model_path.clone(),
                language: language.into(),
                samples_i16_base64: encode_samples_i16_base64(samples),
            },
            cancellation,
        )?;
        match response {
            WorkerResponse::Result {
                request_id: actual_request_id,
                operation_id: actual_operation_id,
                text,
                audio_secs,
                transcribe_secs,
                backend,
                ..
            } if backend == self.backend
                && actual_request_id == request_id
                && actual_operation_id == operation_id =>
            {
                Ok(Transcript {
                    text,
                    detected_language: None,
                    transcribe_secs: Some(transcribe_secs),
                    audio_secs: Some(audio_secs),
                    device: Some(backend_name(backend).into()),
                })
            }
            WorkerResponse::Error { message, .. } => Err(AppError::Stt(format!(
                "{} worker transcription failed: {message}",
                backend_name(self.backend)
            ))),
            other => Err(AppError::Stt(format!(
                "{} worker returned an unexpected transcription result: {other:?}",
                backend_name(self.backend)
            ))),
        }
    }
}
