use super::*;
use fono_stt_protocol::WindowTranscript;

impl WorkerSession {
    pub(super) fn cancel_active(&mut self, target: &WorkerRequest) -> AppError {
        let cancel = WorkerRequest::CancelRequest {
            meta: self.next_meta("cancel", target.meta().operation_id.clone()),
            target_request_id: target.meta().request_id.clone(),
        };
        let Ok(line) = serde_json::to_string(&cancel) else {
            return self.cancel_request();
        };
        let (result_tx, result_rx) = bounded(1);
        let Some(writer) = self.stdin_tx.as_ref() else {
            return self.cancel_request();
        };
        if writer
            .send_timeout(WorkerWrite { line, result_tx }, Duration::from_millis(100))
            .is_err()
            || !matches!(
                result_rx.recv_timeout(Duration::from_millis(100)),
                Ok(Ok(()))
            )
        {
            return self.cancel_request();
        }
        // Native kernels are not assumed to interrupt immediately. If the
        // cooperative response fails to arrive, terminate the isolated process.
        let deadline = Instant::now() + Duration::from_millis(700);
        while Instant::now() < deadline {
            match self.stdout_rx.recv_timeout(CANCELLATION_POLL_INTERVAL) {
                Ok(WorkerOutput::Line(line)) => {
                    let response = serde_json::from_str::<WorkerResponse>(&line);
                    if response.is_ok_and(|response| {
                        response.protocol_version() == PROTOCOL_VERSION
                            && response.request_id() == target.meta().request_id
                    }) {
                        return AppError::Cancelled("worker request cancelled".into());
                    }
                    return self.cancel_request();
                }
                Err(RecvTimeoutError::Timeout) => continue,
                _ => return self.cancel_request(),
            }
        }
        self.cancel_request()
    }

    pub(super) fn transcribe_window(
        &mut self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        operation_id: u64,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
    ) -> AppResult<WindowTranscript> {
        let meta = self.next_meta("window", Some(operation_id.to_string()));
        let response = self.request_with_cancellation(
            &WorkerRequest::TranscribeWindow {
                meta,
                model_path: self.model_path.clone(),
                language: language.into(),
                context: context.map(str::to_owned),
                audio_start_sample,
                samples_i16_base64: encode_samples_i16_base64(samples),
            },
            Some(cancellation),
        )?;
        match response {
            WorkerResponse::WindowResult {
                transcript,
                operation_id: actual,
                ..
            } if transcript.backend == self.backend && actual == operation_id.to_string() => {
                Ok(transcript)
            }
            WorkerResponse::Error { code, message, .. } if code == "cancelled" => {
                Err(AppError::Cancelled(message))
            }
            WorkerResponse::Error { message, .. } => Err(AppError::Stt(message)),
            other => Err(self.fail_request(format!("unexpected window result: {other:?}"))),
        }
    }
}

impl WorkerMailbox {
    pub(in crate::stt) fn transcribe_window(
        &self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        operation_id: u64,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
    ) -> AppResult<WindowTranscript> {
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled("window cancelled before queue".into()));
        }
        let (response_tx, response_rx) = bounded(1);
        self.enqueue(
            WorkerCommand::Window {
                samples: samples.to_vec(),
                language: language.into(),
                context: context.map(str::to_owned),
                operation_id,
                cancellation: cancellation.clone(),
                audio_start_sample,
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
                        "window owner stopped before responding".into(),
                    ))
                }
            }
        }
    }

    pub(super) fn enqueue(
        &self,
        mut command: WorkerCommand,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        loop {
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled("worker queue cancelled".into()));
            }
            match self
                .command_tx
                .send_timeout(command, CANCELLATION_POLL_INTERVAL)
            {
                Ok(()) => return Ok(()),
                Err(SendTimeoutError::Timeout(returned)) => command = returned,
                Err(SendTimeoutError::Disconnected(_)) => {
                    return Err(AppError::Stt("worker mailbox is unavailable".into()))
                }
            }
        }
    }
}
