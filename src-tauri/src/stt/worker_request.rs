impl WorkerSession {
    fn request(&mut self, request: &WorkerRequest) -> AppResult<WorkerResponse> {
        self.request_with_cancellation(request, None)
    }

    fn request_with_cancellation(
        &mut self,
        request: &WorkerRequest,
        cancellation: Option<&OperationCancellation>,
    ) -> AppResult<WorkerResponse> {
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

    fn terminate(&mut self) {
        self.stdin_tx.take();
        terminate_child(&mut self.child);
        if let Some(thread) = self.stdin_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }

    fn request_graceful_shutdown(&mut self) {
        if self.stdin_tx.is_none() || !matches!(self.child.try_wait(), Ok(None)) {
            return;
        }
        let request = WorkerRequest::Shutdown {
            meta: self.next_meta("shutdown", None),
        };
        let Ok(line) = serde_json::to_string(&request) else {
            return;
        };
        let (result_tx, result_rx) = bounded(1);
        let Some(stdin_tx) = self.stdin_tx.as_ref() else {
            return;
        };
        if stdin_tx
            .send_timeout(WorkerWrite { line, result_tx }, Duration::from_millis(500))
            .is_err()
            || !matches!(
                result_rx.recv_timeout(Duration::from_millis(500)),
                Ok(Ok(()))
            )
        {
            return;
        }
        if let Ok(WorkerOutput::Line(line)) =
            self.stdout_rx.recv_timeout(Duration::from_millis(500))
        {
            let acknowledged =
                serde_json::from_str::<WorkerResponse>(&line).is_ok_and(|response| {
                    response.protocol_version() == PROTOCOL_VERSION
                        && response.request_id() == request.meta().request_id
                        && matches!(response, WorkerResponse::ShuttingDown { .. })
                });
            if acknowledged {
                let _ = self.child.wait();
            }
        }
    }
}
