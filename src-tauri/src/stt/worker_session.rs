impl WorkerSession {
    pub(super) fn start(
        path: PathBuf,
        backend: BackendKind,
        model_path: &Path,
        cleanup: Arc<WorkerCleanupSupervisor>,
        package: Option<crate::application::cuda_packages::VerifiedPackage>,
    ) -> AppResult<Self> {
        Self::start_owned(
            path,
            backend,
            model_path,
            PRODUCTION_TIMEOUTS,
            cleanup,
            package,
        )
    }

    #[cfg(test)]
    fn start_with_timeouts(
        path: PathBuf,
        backend: BackendKind,
        model_path: &Path,
        timeouts: WorkerTimeouts,
    ) -> AppResult<Self> {
        Self::start_with_timeouts_and_supervisor(
            path,
            backend,
            model_path,
            timeouts,
            Arc::default(),
        )
    }

    #[cfg(test)]
    fn start_with_timeouts_and_supervisor(
        path: PathBuf,
        backend: BackendKind,
        model_path: &Path,
        timeouts: WorkerTimeouts,
        cleanup: Arc<WorkerCleanupSupervisor>,
    ) -> AppResult<Self> {
        Self::start_owned(path, backend, model_path, timeouts, cleanup, None)
    }

    fn start_owned(
        path: PathBuf,
        backend: BackendKind,
        model_path: &Path,
        timeouts: WorkerTimeouts,
        cleanup: Arc<WorkerCleanupSupervisor>,
        package: Option<crate::application::cuda_packages::VerifiedPackage>,
    ) -> AppResult<Self> {
        let mut worker = Self::connect(path, backend, model_path, timeouts, cleanup, package)?;
        let load_meta = worker.next_meta("load", None);
        match worker.request(&WorkerRequest::Load {
            meta: load_meta,
            model_path: worker.model_path.clone(),
        })? {
            WorkerResponse::ModelLoaded { backend: actual, .. } if actual == backend => {}
            WorkerResponse::Error { message, .. } => {
                return Err(AppError::Stt(format!(
                    "{} worker could not load model: {message}", backend_name(backend)
                )))
            }
            other => return Err(AppError::Stt(format!(
                "{} worker returned an unexpected load result: {other:?}", backend_name(backend)
            ))),
        }
        tracing::info!("{} worker loaded the Whisper model", backend_name(backend));
        Ok(worker)
    }

    fn connect(
        path: PathBuf,
        backend: BackendKind,
        model_path: &Path,
        timeouts: WorkerTimeouts,
        cleanup: Arc<WorkerCleanupSupervisor>,
        package: Option<crate::application::cuda_packages::VerifiedPackage>,
    ) -> AppResult<Self> {
        if cleanup.is_pending() {
            return Err(worker_cleanup_busy());
        }
        if let Some(package) = package.as_ref() {
            if path != package.worker_path() {
                return Err(AppError::Config("CUDA package worker path differs from its verified manifest".into()));
            }
            // Signed files and ancestors remain locked. Directory handles do
            // not prevent adding a new child; reject unexpected DLLs again at
            // the last boundary before Windows resolves the worker imports.
            package.validate_layout()?;
        }
        let mut command = Command::new(&path);
        if package.is_some() {
            command.current_dir(path.parent().ok_or_else(|| AppError::Config("CUDA package directory is unavailable".into()))?);
        }
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
            terminate_unstarted_child(child, &cleanup, package);
            return Err(AppError::Stt("worker stdin is unavailable".into()));
        };
        let Some(stdout) = child.stdout.take() else {
            terminate_unstarted_child(child, &cleanup, package);
            return Err(AppError::Stt("worker stdout is unavailable".into()));
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_unstarted_child(child, &cleanup, package);
            return Err(AppError::Stt("worker stderr is unavailable".into()));
        };
        let child = Arc::new(WorkerChild::new(child, package));
        let transport_stopping = Arc::new(AtomicBool::new(false));
        let (stdout_rx, stdout_thread) =
            spawn_stdout_reader(stdout, Arc::clone(&transport_stopping));
        let (stdin_tx, stdin_thread) = spawn_stdin_writer(stdin, Arc::clone(&transport_stopping));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        let stderr_thread =
            spawn_stderr_reader(stderr, stderr_tail.clone(), Arc::clone(&transport_stopping));
        let mut worker = Self {
            backend,
            path,
            model_path: model_path.to_string_lossy().to_string(),
            child,
            cleanup,
            transport_stopping,
            cleanup_ticket: None,
            stdin_tx: Some(stdin_tx),
            stdin_thread: Some(stdin_thread),
            stdout_rx,
            stdout_thread: Some(stdout_thread),
            stderr_tail,
            stderr_thread: Some(stderr_thread),
            next_request_id: 0,
            timeouts,
            supports_decoding_options: false,
            #[cfg(test)]
            written_frames: Arc::new(AtomicUsize::new(0)),
        };
        let hello_meta = worker.next_meta("hello", None);
        match worker.request(&WorkerRequest::Hello { meta: hello_meta })? {
            WorkerResponse::Ready {
                backend: actual,
                capabilities,
                ..
            } if actual == backend
                && capabilities.protocol_version == PROTOCOL_VERSION
                && capabilities.supports_health
                && capabilities.supports_shutdown
                && capabilities.supports_window
                && capabilities.supports_cancel
                && capabilities.supports_token_timestamps
                && capabilities.maximum_request_bytes >= MAX_REQUEST_FRAME_BYTES
                && capabilities.maximum_response_bytes >= MAX_RESPONSE_FRAME_BYTES => { worker.supports_decoding_options = capabilities.supports_decoding_options; }
            other => {
                return Err(AppError::Stt(format!(
                    "{} worker returned an unexpected handshake: {other:?}",
                    backend_name(backend)
                )))
            }
        }
        Ok(worker)
    }

    pub(super) fn probe(
        path: PathBuf,
        package: Option<crate::application::cuda_packages::VerifiedPackage>,
    ) -> AppResult<()> {
        let mut worker = Self::connect(
            path, BackendKind::Cuda, Path::new(""), PRODUCTION_TIMEOUTS,
            Arc::default(), package,
        )?;
        if worker.shutdown() != WorkerCleanupStatus::Exited {
            return Err(worker_cleanup_busy());
        }
        Ok(())
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
            other => Err(self.fail_request(format!(
                "{} worker returned an unexpected health response: {other:?}",
                backend_name(self.backend)
            ))),
        }
    }

    #[cfg(test)]
    pub(super) fn transcribe(&mut self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_with_cancellation(samples, language, None, fono_stt_protocol::DecodingOptions::default())
    }

    fn validate_decoding(&self, decoding: fono_stt_protocol::DecodingOptions) -> AppResult<()> {
        decoding.validate().map_err(AppError::Config)?;
        if !decoding.is_default() && !self.supports_decoding_options {
            return Err(AppError::Config("Worker does not support the selected Whisper decoding options; rebuild or update the worker".into()));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn transcribe_cancellable(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe_cancellable_with_options(samples, language, cancellation, fono_stt_protocol::DecodingOptions::default())
    }

    pub(super) fn transcribe_cancellable_with_options(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
        decoding: fono_stt_protocol::DecodingOptions,
    ) -> AppResult<Transcript> {
        self.transcribe_with_cancellation(samples, language, Some(cancellation), decoding)
    }

    fn transcribe_with_cancellation(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: Option<&OperationCancellation>,
        decoding: fono_stt_protocol::DecodingOptions,
    ) -> AppResult<Transcript> {
        self.validate_decoding(decoding)?;
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
                decoding,
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
            other => Err(self.fail_request(format!(
                "{} worker returned an unexpected transcription result: {other:?}",
                backend_name(self.backend)
            ))),
        }
    }
}
