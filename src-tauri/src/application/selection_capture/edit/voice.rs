//! Reuse the isolated raw capture: AudioHub, speech VAD, scheduler and no archive.
use super::*;
use crate::application::dictation;

async fn native<T: Send + 'static>(
    work: &Work,
    action: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let lease = work._lease.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        action()
    })
    .await
    .map_err(|_| AppError::Internal("Не удалось завершить запись инструкции.".into()))?
}
async fn cancel_capture(app: &AppHandle, work: &Work, capture: u64) -> AppResult<()> {
    let app = app.clone();
    native(work, move || {
        dictation::cancel_processing_test_capture(&app, capture)
    })
    .await
}
pub(super) async fn run(app: &AppHandle, work: &Work) -> AppResult<String> {
    if work.control.cancel.is_cancelled() {
        return Err(stale());
    }
    let start_app = app.clone();
    let started = native(work, move || {
        dictation::start_processing_test_capture(start_app)
    })
    .await?;
    let capture = started.session_id;
    if work.control.cancel.is_cancelled() {
        cancel_capture(app, work, capture).await?;
        return Err(stale());
    }
    phase(app, work, "recording");
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(120));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            _ = &mut timeout => {
                cancel_capture(app, work, capture).await?;
                return Err(AppError::Cancelled("Время записи инструкции истекло. Повторите запись.".into()));
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
        }
        if work.control.cancel.is_cancelled() {
            cancel_capture(app, work, capture).await?;
            return Err(stale());
        }
        if work.control.stop.load(Ordering::Acquire) {
            break;
        }
    }
    phase(app, work, "recognizing");
    let result = tokio::select! {
        result = dictation::finish_processing_test_capture(app.clone(), capture) => result.map(|v| v.text),
        _ = dictation::wait_for_cancellation(work.control.cancel.clone()) => {
            cancel_capture(app, work, capture).await?;
            return Err(stale());
        }
    }?;
    validate_instruction(&result).map_err(|_| {
        AppError::Config(
            "Не удалось распознать инструкцию. Повторите запись или введите её вручную.".into(),
        )
    })?;
    Ok(result)
}
