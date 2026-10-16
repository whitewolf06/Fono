//! Native insertion checks the original destination at every send boundary.
use super::{Runtime, Session, TextTarget};
use crate::{
    application::updates::activity,
    error::{AppError, AppResult},
    injection::target::AppendResult,
};
use fono_core::OperationCancellation;
use tauri::Manager;

pub(super) async fn insert(
    session: &Session,
    target: Option<&TextTarget>,
    text: &str,
) -> AppResult<()> {
    let target = target
        .cloned()
        .ok_or_else(|| {
            AppError::Injection(
                "Поле для вставки изменилось или недоступно. Скопируйте текст из Fono или начните новую диктовку в нужном поле".into(),
            )
        })?;
    let activity = activity::lease()?;
    let validity = session
        .app
        .state::<Runtime>()
        .capture_target
        .lock()
        .claim(session.operation, &target)?;
    let attempt = CancelAttempt(OperationCancellation::default());
    let text = text.to_owned();
    let expected = text.len();
    let cancellation = session
        .cancellation
        .combined_with(&validity)
        .combined_with(&attempt.0);
    let mode = session.settings.injection_mode;
    let output = tokio::task::spawn_blocking(move || {
        let _activity = activity;
        crate::injection::target::append(&target, &text, mode, &cancellation)
    })
    .await
    .map_err(|error| AppError::Injection(format!("Вставка прервана: {error}")))??;
    validate_completion(output, expected)
}

/// A dropped IPC future cancels its native send, but its blocking task and
/// clipboard restoration retain their admission leases until actual completion.
struct CancelAttempt(OperationCancellation);
impl Drop for CancelAttempt {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn validate_completion(output: AppendResult, expected: usize) -> AppResult<()> {
    if !output.paused && !output.uncertain && output.bytes == expected {
        return Ok(());
    }
    Err(AppError::Injection("Вставка остановлена или отправлена частично. Проверьте поле; полный текст доступен в Fono для копирования".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abandoned_ipc_cancels_its_worker_without_cancelling_the_session() {
        let session = OperationCancellation::default();
        let validity = OperationCancellation::default();
        let attempt = CancelAttempt(OperationCancellation::default());
        let worker = session.combined_with(&validity).combined_with(&attempt.0);
        assert!(!worker.is_cancelled());
        drop(attempt);
        assert!(worker.is_cancelled());
        assert!(!session.is_cancelled());
        assert!(!validity.is_cancelled());
    }

    #[test]
    fn partial_paused_and_uncertain_sends_never_claim_success() {
        for output in [
            AppendResult {
                bytes: 2,
                paused: true,
                uncertain: false,
            },
            AppendResult {
                bytes: 4,
                paused: false,
                uncertain: true,
            },
            AppendResult {
                bytes: 2,
                paused: false,
                uncertain: false,
            },
        ] {
            assert!(validate_completion(output, 4).is_err());
        }
        assert!(validate_completion(
            AppendResult {
                bytes: 4,
                paused: false,
                uncertain: false
            },
            4
        )
        .is_ok());
    }
}
