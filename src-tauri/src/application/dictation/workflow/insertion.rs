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
    if target.is_none() {
        super::observe_context(
            &session.app,
            session.operation,
            crate::types::ApplicationContext::unknown_attempt(session.settings.injection_mode),
        );
    }
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
    let app = session.app.clone();
    let operation = session.operation;
    let context_epoch = super::application_context_for(&app, operation).map(|v| v.0);
    let output = tokio::task::spawn_blocking(move || {
        let _activity = activity;
        let tracking = context_epoch.is_some()
            && crate::history::for_app(&app).application_context_permit() == context_epoch;
        let identity = || {
            crate::injection::target::executable_name(&target)
                .as_deref()
                .and_then(crate::types::ApplicationIdentity::from_executable)
        };
        let before = tracking.then(&identity).flatten();
        if tracking {
            super::observe_context(
                &app,
                operation,
                crate::types::ApplicationContext::unknown_attempt(mode),
            );
        }
        let result = crate::injection::target::append(&target, &text, mode, &cancellation);
        if tracking {
            let after = identity();
            super::observe_context(
                &app,
                operation,
                observed_context(before, after, mode, &result, expected),
            );
        }
        result
    })
    .await
    .map_err(|error| AppError::Injection(format!("Вставка прервана: {error}")))??;
    validate_completion(output, expected)
}

fn observed_context(
    before: Option<crate::types::ApplicationIdentity>,
    after: Option<crate::types::ApplicationIdentity>,
    method: crate::types::InjectionMode,
    result: &AppResult<AppendResult>,
    expected: usize,
) -> crate::types::ApplicationContext {
    use crate::types::InsertionObservation as O;
    let changed = before
        .as_ref()
        .is_some_and(|a| after.as_ref().map_or(true, |b| a.id != b.id));
    let outcome = if changed || result.as_ref().is_ok_and(|v| v.paused) {
        O::FocusChanged
    } else if result
        .as_ref()
        .is_ok_and(|v| v.uncertain || v.bytes != expected)
    {
        O::Partial
    } else if result.is_err() {
        O::Failed
    } else if before.is_none() {
        O::Unknown
    } else {
        O::Sent
    };
    // Never attribute to an application first discovered after sending.
    let after = if before.is_some() && !changed {
        after
    } else {
        None
    };
    crate::types::ApplicationContext {
        before,
        after,
        outcome,
        method: Some(method),
        attempted: true,
    }
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
    fn observation_never_promises_delivery_or_assigns_the_new_foreground() {
        use crate::types::{
            ApplicationIdentity as A, InjectionMode as M, InsertionObservation as O,
        };
        let before = || A::from_executable("Code.exe");
        let ok = Ok(AppendResult {
            bytes: 4,
            paused: false,
            uncertain: false,
        });
        assert_eq!(
            observed_context(before(), before(), M::Clipboard, &ok, 4).outcome,
            O::Sent
        );
        let changed = observed_context(
            before(),
            A::from_executable("Other.exe"),
            M::Clipboard,
            &ok,
            4,
        );
        assert_eq!(changed.outcome, O::FocusChanged);
        assert!(changed.after.is_none());
        assert_eq!(changed.before.unwrap().id, "exe:code.exe");
        assert!(observed_context(None, before(), M::SendInput, &ok, 4)
            .after
            .is_none());
        assert_eq!(
            observed_context(None, None, M::SendInput, &ok, 4).outcome,
            O::Unknown
        );
        let partial = Ok(AppendResult {
            bytes: 2,
            paused: false,
            uncertain: true,
        });
        assert_eq!(
            observed_context(before(), before(), M::SendInput, &partial, 4).outcome,
            O::Partial
        );
        assert_eq!(
            observed_context(
                before(),
                before(),
                M::Clipboard,
                &Err(AppError::Injection("opaque".into())),
                4
            )
            .outcome,
            O::Failed
        );
    }
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
