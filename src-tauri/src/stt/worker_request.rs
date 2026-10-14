impl WorkerSession {
    fn request(&mut self, request: &WorkerRequest) -> AppResult<WorkerResponse> {
        self.request_with_cancellation(request, None)
    }

    fn request_with_cancellation(
        &mut self,
        request: &WorkerRequest,
        cancellation: Option<&OperationCancellation>,
    ) -> AppResult<WorkerResponse> {
        let _activity = crate::application::updates::activity::lease()?;
        let timeout = self.timeouts.for_request(request);
        let deadline = Instant::now() + timeout;
        if cancellation.is_some_and(OperationCancellation::is_cancelled) {
            return Err(AppError::Cancelled(
                "worker request cancelled before transport".into(),
            ));
        }
        let line = serde_json::to_string(request)
            .map_err(|error| AppError::Stt(format!("worker request serialization: {error}")))?;
        if line.len() > MAX_REQUEST_FRAME_BYTES {
            return Err(self.fail_request(format!(
                "worker request exceeds {MAX_REQUEST_FRAME_BYTES} bytes"
            )));
        }
        let (result_tx, result_rx) = bounded(1);
        let write = WorkerWrite { line, result_tx };
        let Some(stdin_tx) = self.stdin_tx.as_ref() else {
            return Err(self.fail_request("worker stdin is closed".into()));
        };
        if let Err(error) = stdin_tx.send_timeout(write, remaining_until(deadline)) {
            let message = match error {
                SendTimeoutError::Timeout(_) => "worker stdin queue timed out",
                SendTimeoutError::Disconnected(_) => "worker stdin writer disconnected",
            };
            return Err(self.fail_request(message.into()));
        }
        loop {
            if self.child.stopping.load(Ordering::Acquire) {
                return Err(self.cancel_request());
            }
            if cancellation.is_some_and(OperationCancellation::is_cancelled) {
                // A partially written JSON frame cannot be safely interrupted in-band.
                return Err(self.cancel_request());
            }
            let remaining = remaining_until(deadline);
            if remaining.is_zero() {
                return Err(self.fail_request("worker stdin write timed out".into()));
            }
            match result_rx.recv_timeout(remaining.min(CANCELLATION_POLL_INTERVAL)) {
                Ok(Ok(())) => break,
                Ok(Err(error)) => return Err(self.fail_request(format!("worker stdin: {error}"))),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.fail_request("worker stdin writer disconnected".into()))
                }
            }
        }
        let response = loop {
            if self.child.stopping.load(Ordering::Acquire) {
                return Err(self.cancel_request());
            }
            if cancellation.is_some_and(OperationCancellation::is_cancelled) {
                return Err(self.cancel_active(request));
            }
            let remaining = remaining_until(deadline);
            if remaining.is_zero() {
                return Err(self.fail_request(format!(
                    "{} worker request timed out after {:.1}s",
                    backend_name(self.backend),
                    timeout.as_secs_f32()
                )));
            }
            match self
                .stdout_rx
                .recv_timeout(remaining.min(CANCELLATION_POLL_INTERVAL))
            {
                Ok(WorkerOutput::Line(response)) => break response,
                Ok(WorkerOutput::Eof) => {
                    return Err(
                        self.fail_request("worker exited before returning a response".into())
                    )
                }
                Ok(WorkerOutput::Error(error)) => {
                    return Err(self.fail_request(format!("worker stdout: {error}")))
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.fail_request("worker stdout reader disconnected".into()))
                }
            }
        };

        let response: WorkerResponse = serde_json::from_str(response.trim())
            .map_err(|error| self.fail_request(format!("worker returned invalid JSON: {error}")))?;
        if response.protocol_version() != PROTOCOL_VERSION {
            return Err(self.fail_request(format!(
                "worker returned protocol version {}; expected {PROTOCOL_VERSION}",
                response.protocol_version()
            )));
        }
        if response.request_id() != request.meta().request_id {
            return Err(self.fail_request(format!(
                "worker returned request_id {}; expected {}",
                response.request_id(),
                request.meta().request_id
            )));
        }
        Ok(response)
    }

    fn next_meta(&mut self, kind: &str, operation_id: Option<String>) -> RequestMeta {
        self.next_request_id = self.next_request_id.wrapping_add(1);
        RequestMeta::new(format!("{kind}-{}", self.next_request_id), operation_id)
    }

    #[cfg(test)]
    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
            && self
                .stdin_thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
            && self
                .stdout_thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
            && self
                .stderr_thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
    }

    fn fail_request(&mut self, message: String) -> AppError {
        self.terminate();
        let diagnostics = self.stderr_diagnostics();
        if diagnostics.is_empty() {
            AppError::Stt(message)
        } else {
            AppError::Stt(format!("{message}; worker stderr: {diagnostics}"))
        }
    }

    fn cancel_request(&mut self) -> AppError {
        let error = self.fail_request("worker request cancelled".into());
        AppError::Cancelled(error.to_string())
    }

    fn stderr_diagnostics(&self) -> String {
        self.stderr_tail
            .lock()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn terminate(&mut self) -> WorkerCleanupStatus {
        if let Some(ticket) = &self.cleanup_ticket {
            return if ticket.load(Ordering::Acquire) {
                WorkerCleanupStatus::Exited
            } else {
                WorkerCleanupStatus::Pending
            };
        }
        let activity = Some(crate::application::updates::activity::cleanup_lease());
        let _fence = self.cleanup.begin();
        self.child.stopping.store(true, Ordering::Release);
        self.transport_stopping.store(true, Ordering::Release);
        self.stdin_tx.take();
        let exited = self.child.force_exit(Instant::now() + FORCE_EXIT_TIMEOUT);
        let handles = [
            self.stdin_thread.take(),
            self.stdout_thread.take(),
            self.stderr_thread.take(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let handles = bounded_join(handles, Instant::now() + THREAD_JOIN_TIMEOUT);
        if exited && handles.is_empty() {
            return WorkerCleanupStatus::Exited;
        }
        self.cleanup_ticket = Some(self.cleanup.retire(
            Arc::clone(&self.child),
            handles,
            Arc::clone(&self.transport_stopping),
            activity,
        ));
        WorkerCleanupStatus::Pending
    }

    pub(super) fn shutdown(&mut self) -> WorkerCleanupStatus {
        if self.cleanup_ticket.is_some() {
            return self.terminate();
        }
        let _activity = crate::application::updates::activity::cleanup_lease();
        let _fence = self.cleanup.begin();
        self.request_graceful_shutdown();
        self.terminate()
    }

    fn request_graceful_shutdown(&mut self) {
        if self.stdin_tx.is_none() || !matches!(self.child.try_wait(), Ok(None)) {
            return;
        }
        let request = WorkerRequest::Shutdown {
            meta: self.next_meta("shutdown", None),
        };
        let deadline = Instant::now() + GRACEFUL_SHUTDOWN_TIMEOUT;
        let Ok(line) = serde_json::to_string(&request) else {
            return;
        };
        let (result_tx, result_rx) = bounded(1);
        let Some(stdin_tx) = self.stdin_tx.as_ref() else {
            return;
        };
        if stdin_tx
            .send_timeout(WorkerWrite { line, result_tx }, remaining_until(deadline))
            .is_err()
            || !matches!(
                result_rx.recv_timeout(remaining_until(deadline)),
                Ok(Ok(()))
            )
        {
            return;
        }
        if let Ok(WorkerOutput::Line(line)) = self.stdout_rx.recv_timeout(remaining_until(deadline))
        {
            let acknowledged =
                serde_json::from_str::<WorkerResponse>(&line).is_ok_and(|response| {
                    response.protocol_version() == PROTOCOL_VERSION
                        && response.request_id() == request.meta().request_id
                        && matches!(response, WorkerResponse::ShuttingDown { .. })
                });
            if acknowledged {
                self.child.await_exit(deadline);
            }
        }
    }
}
