fn encode_samples_i16_base64(samples: &[i16]) -> String {
    let mut pcm = Vec::with_capacity(std::mem::size_of_val(samples));
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(pcm)
}

impl WorkerMailbox {
    pub(super) fn is_idle(&self) -> bool {
        !self.busy.load(Ordering::Acquire)
            && !self.health_pending.load(Ordering::Acquire)
            && self.command_tx.is_empty()
    }

    pub(super) fn start(session: WorkerSession) -> Self {
        let backend = session.backend;
        let path = session.path.clone();
        let model_path = session.model_path.clone();
        let (command_tx, command_rx) = bounded(2);
        let busy = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let thread_busy = Arc::clone(&busy);
        let thread_alive = Arc::clone(&alive);
        let owner_thread = thread::spawn(move || {
            let mut session = session;
            while let Ok(command) = command_rx.recv() {
                let keep_running = match command {
                    WorkerCommand::Window {
                        samples,
                        language,
                        context,
                        operation_id,
                        cancellation,
                        audio_start_sample,
                        response_tx,
                    } => {
                        thread_busy.store(true, Ordering::Release);
                        let result = session.transcribe_window(
                            &samples,
                            &language,
                            context.as_deref(),
                            operation_id,
                            &cancellation,
                            audio_start_sample,
                        );
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = session.stdin_tx.is_some();
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Transcribe {
                        samples,
                        language,
                        cancellation,
                        response_tx,
                    } => {
                        thread_busy.store(true, Ordering::Release);
                        let result =
                            session.transcribe_cancellable(&samples, &language, &cancellation);
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = session.stdin_tx.is_some();
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Ping { response_tx } => {
                        thread_busy.store(true, Ordering::Release);
                        let result = session.ping();
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = result.is_ok();
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Shutdown => false,
                };
                if !keep_running {
                    break;
                }
            }
            thread_busy.store(false, Ordering::Release);
            thread_alive.store(false, Ordering::Release);
        });
        Self {
            backend,
            path,
            model_path,
            command_tx,
            owner_thread: Mutex::new(Some(owner_thread)),
            busy,
            alive,
            health_pending: AtomicBool::new(false),
        }
    }

    pub(super) fn compatible_with(
        &self,
        model_path: &str,
        backend: BackendKind,
        path: &Path,
    ) -> bool {
        self.model_path == model_path
            && self.backend == backend
            && self.path == path
            && self.alive.load(Ordering::Acquire)
    }

    pub(super) fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_cancellable(samples, language, &OperationCancellation::default())
    }

    pub(super) fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled(
                "worker transcription cancelled before start".into(),
            ));
        }
        let (response_tx, response_rx) = bounded(1);
        self.enqueue(
            WorkerCommand::Transcribe {
                samples: samples.to_vec(),
                language: language.into(),
                cancellation: cancellation.clone(),
                response_tx,
            },
            cancellation,
        )?;
        loop {
            match response_rx.recv_timeout(CANCELLATION_POLL_INTERVAL) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(AppError::Stt(
                        "worker mailbox stopped before responding".into(),
                    ))
                }
            }
        }
    }

    pub(super) fn health(&self) -> MailboxHealth {
        if self.busy.load(Ordering::Acquire)
            || self
                .health_pending
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return MailboxHealth::Busy;
        }
        let (response_tx, response_rx) = bounded(1);
        let send = self
            .command_tx
            .try_send(WorkerCommand::Ping { response_tx });
        if send.is_err() {
            self.health_pending.store(false, Ordering::Release);
            return MailboxHealth::Failed(AppError::Stt("worker mailbox is unavailable".into()));
        }
        let health = match response_rx.recv_timeout(MAILBOX_HEALTH_TIMEOUT) {
            Ok(Ok(())) => MailboxHealth::Ready,
            Ok(Err(error)) => MailboxHealth::Failed(error),
            Err(RecvTimeoutError::Timeout) => MailboxHealth::Busy,
            Err(RecvTimeoutError::Disconnected) => MailboxHealth::Failed(AppError::Stt(
                "worker mailbox stopped during health probe".into(),
            )),
        };
        self.health_pending.store(false, Ordering::Release);
        health
    }
}

impl Drop for WorkerMailbox {
    fn drop(&mut self) {
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        if let Some(thread) = self.owner_thread.lock().take() {
            let _ = thread.join();
        }
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        self.request_graceful_shutdown();
        self.terminate();
    }
}
