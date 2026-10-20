impl Recognizer for WorkerMailbox {
    fn load(request: &RecognizerLoad<'_>) -> AppResult<Self> {
        let super::EngineCandidate::Worker { backend, path, package } = request.candidate else {
            return Err(AppError::Config("Expected a worker recognizer".into()));
        };
        let session = WorkerSession::start(
            path.clone(),
            *backend,
            Path::new(&request.model.local_path),
            request.cleanup.clone(),
            package.as_deref().cloned(),
        )?;
        let mut mailbox = Self::start(session);
        mailbox.model = request.model.id.clone();
        Ok(mailbox)
    }

    fn model(&self) -> &ModelId {
        &self.model
    }

    fn device(&self) -> &'static str {
        backend_name(self.backend)
    }

    fn capabilities(&self) -> RecognizerCapabilities {
        RecognizerCapabilities::whisper(self.supports_decoding_options)
    }

    fn transcribe(
        &self,
        samples: &[i16],
        options: RecognitionOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe_cancellable_with_options(
            samples, options.language, cancellation, options.decoding, options.glossary,
        )
    }

    fn transcribe_window(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<fono_stt_protocol::WindowTranscript> {
        self.transcribe_window(
            samples,
            options.recognition.language,
            options.context,
            options.operation_id,
            cancellation,
            options.audio_start_sample,
            options.recognition.decoding,
            options.recognition.glossary,
        )
    }
}

fn encode_samples_i16_base64(samples: &[i16]) -> String {
    let mut pcm = Vec::with_capacity(std::mem::size_of_val(samples));
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(pcm)
}

impl WorkerMailbox {
    pub(super) fn is_idle(&self) -> bool {
        !self.closing.load(Ordering::Acquire)
            && !self.cleanup.is_pending()
            && !self.busy.load(Ordering::Acquire)
            && !self.health_pending.load(Ordering::Acquire)
            && self.command_tx.is_empty()
            && !self.fault.is_faulted()
    }

    pub(super) fn start(session: WorkerSession) -> Self {
        let backend = session.backend;
        let path = session.path.clone();
        let model_path = session.model_path.clone();
        let model = SttModelSelection::from_whisper_path(model_path.clone()).id;
        let supports_decoding_options = session.supports_decoding_options;
        let child = Arc::clone(&session.child);
        let cleanup = Arc::clone(&session.cleanup);
        let transport_stopping = Arc::clone(&session.transport_stopping);
        let (command_tx, command_rx) = bounded(2);
        let busy = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let thread_busy = Arc::clone(&busy);
        let thread_alive = Arc::clone(&alive);
        let fault = Arc::new(super::recovery::FaultSignal::default());
        let owner_fault = fault.clone();
        let owner_thread = thread::spawn(move || {
            let mut session = session;
            loop {
                let command = crossbeam_channel::select_biased! {
                    recv(command_rx) -> command => match command {
                        Ok(command) => command,
                        Err(_) => break,
                    },
                    recv(session.stdout_rx) -> _output => {
                        // Between requests any stdout line/EOF/error is a
                        // protocol/transport fault. Shutdown suppresses first.
                        owner_fault.fault();
                        break;
                    }
                };
                let keep_running = match command {
                    WorkerCommand::Window {
                        samples,
                        language,
                        decoding,
                        glossary,
                        context,
                        operation_id,
                        cancellation,
                        audio_start_sample,
                        response_tx,
                        activity,
                    } => {
                        let _activity = activity;
                        thread_busy.store(true, Ordering::Release);
                        let result = session.transcribe_window(
                            &samples,
                            &language,
                            context.as_deref(),
                            operation_id,
                            &cancellation,
                            audio_start_sample,
                            decoding,
                            &glossary,
                        );
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = session.stdin_tx.is_some();
                        if !keep_running {
                            if matches!(&result, Err(AppError::Cancelled(_))) {
                                owner_fault.suppress();
                            } else {
                                owner_fault.fault();
                            }
                        }
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Transcribe {
                        samples,
                        language,
                        decoding,
                        glossary,
                        cancellation,
                        response_tx,
                        activity,
                    } => {
                        let _activity = activity;
                        thread_busy.store(true, Ordering::Release);
                        let result =
                            session.transcribe_cancellable_with_options(&samples, &language, &cancellation, decoding, &glossary);
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = session.stdin_tx.is_some();
                        if !keep_running {
                            if matches!(&result, Err(AppError::Cancelled(_))) {
                                owner_fault.suppress();
                            } else {
                                owner_fault.fault();
                            }
                        }
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Ping {
                        response_tx,
                        activity,
                    } => {
                        let _activity = activity;
                        thread_busy.store(true, Ordering::Release);
                        let result = session.ping();
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = result.is_ok();
                        if !keep_running {
                            owner_fault.fault();
                            session.shutdown();
                        }
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Shutdown { response_tx } => {
                        owner_fault.suppress();
                        let status = session.shutdown();
                        let _ = response_tx.try_send(status);
                        false
                    }
                };
                if !keep_running {
                    break;
                }
            }
            drop(session);
            thread_busy.store(false, Ordering::Release);
            thread_alive.store(false, Ordering::Release);
        });
        Self {
            backend,
            path,
            model_path,
            model,
            supports_decoding_options,
            command_tx,
            owner_thread: Mutex::new(Some(owner_thread)),
            busy,
            alive,
            health_pending: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            child,
            cleanup,
            transport_stopping,
            fault,
        }
    }

    pub(super) fn arm_fault(&self, observer: super::recovery::FaultObserver) {
        self.fault.arm(observer);
    }
    pub(super) fn is_faulted(&self) -> bool {
        self.fault.is_faulted()
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
            && !self.closing.load(Ordering::Acquire)
            && !self.child.stopping.load(Ordering::Acquire)
            && !self.cleanup.is_pending()
            && !self.fault.is_faulted()
    }

    pub(super) fn has_resources(&self) -> bool {
        !self.child.exited() || self.cleanup.is_pending()
    }

    pub(super) fn is_retired(&self) -> bool {
        (self.closing.load(Ordering::Acquire) || self.child.stopping.load(Ordering::Acquire))
            && !self.has_resources()
            && !self.alive.load(Ordering::Acquire)
            && self
                .owner_thread
                .lock()
                .as_ref()
                .map_or(true, JoinHandle::is_finished)
    }

    #[cfg(test)]
    pub(super) fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_cancellable(samples, language, &OperationCancellation::default())
    }

    #[cfg(test)]
    pub(super) fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe_cancellable_with_options(samples, language, cancellation, fono_stt_protocol::DecodingOptions::default(), &[])
    }

    pub(super) fn transcribe_cancellable_with_options(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: &[String],
    ) -> AppResult<Transcript> {
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled(
                "worker transcription cancelled before start".into(),
            ));
        }
        let (response_tx, response_rx) = bounded(1);
        let activity = crate::application::updates::activity::lease()?;
        self.enqueue(
            WorkerCommand::Transcribe {
                samples: samples.to_vec(),
                language: language.into(),
                decoding,
                glossary: glossary.to_vec(),
                cancellation: cancellation.clone(),
                response_tx,
                activity,
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
        if self.closing.load(Ordering::Acquire) || self.cleanup.is_pending() {
            return MailboxHealth::Busy;
        }
        let Ok(activity) = crate::application::updates::activity::lease() else {
            return MailboxHealth::Busy;
        };
        if self.busy.load(Ordering::Acquire)
            || self
                .health_pending
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return MailboxHealth::Busy;
        }
        let (response_tx, response_rx) = bounded(1);
        let send = self.command_tx.try_send(WorkerCommand::Ping {
            response_tx,
            activity,
        });
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

    pub(super) fn shutdown(&self) -> WorkerCleanupStatus {
        self.fault.suppress();
        if self.is_retired() {
            if let Some(owner) = self.owner_thread.lock().take() {
                let _ = owner.join();
            }
            return WorkerCleanupStatus::Exited;
        }
        let activity = Some(crate::application::updates::activity::cleanup_lease());
        let _fence = self.cleanup.begin();
        self.closing.store(true, Ordering::Release);
        let Some(owner) = self.owner_thread.lock().take() else {
            // This fence itself contributes one pending entry.
            return if self.child.exited()
                && self.cleanup.pending.load(Ordering::Acquire) == 1
                && !self.alive.load(Ordering::Acquire)
            {
                WorkerCleanupStatus::Exited
            } else {
                WorkerCleanupStatus::Pending
            };
        };
        // Interrupt a busy request in its polling loop; an idle owner still
        // gets a normal protocol Shutdown. No caller waits on inference.
        self.child.stopping.store(true, Ordering::Release);
        let (response_tx, response_rx) = bounded(1);
        let deadline = Instant::now() + MAILBOX_SHUTDOWN_TIMEOUT;
        let sent = self
            .command_tx
            .send_timeout(
                WorkerCommand::Shutdown { response_tx },
                remaining_until(deadline),
            )
            .is_ok();
        if sent {
            let _ = response_rx.recv_timeout(remaining_until(deadline));
        }
        let mut handles = bounded_join(vec![owner], deadline);
        if !handles.is_empty() || !self.child.exited() {
            self.child.stopping.store(true, Ordering::Release);
            self.transport_stopping.store(true, Ordering::Release);
            self.child.force_exit(Instant::now() + FORCE_EXIT_TIMEOUT);
            handles = bounded_join(handles, Instant::now() + THREAD_JOIN_TIMEOUT);
        }
        if self.child.exited()
            && handles.is_empty()
            && self.cleanup.pending.load(Ordering::Acquire) == 1
        {
            WorkerCleanupStatus::Exited
        } else {
            if !handles.is_empty() || !self.child.exited() {
                self.cleanup.retire(
                    Arc::clone(&self.child),
                    handles,
                    Arc::clone(&self.transport_stopping),
                    activity,
                );
            }
            WorkerCleanupStatus::Pending
        }
    }
}

impl Drop for WorkerMailbox {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}
