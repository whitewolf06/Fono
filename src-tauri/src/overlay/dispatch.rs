//! Window work always starts on the UI thread, after an asynchronous hop.
//! Tauri's run_on_main_thread executes inline when called on that thread, so
//! submitting directly from a domain fence would reintroduce lock inversion.
use crate::error::{AppError, AppResult};
use tauri::AppHandle;

type UiTask = Box<dyn FnOnce() + Send>;

pub(super) trait Scheduler: Send + 'static {
    fn submit(&self, task: UiTask) -> AppResult<()>;
}

impl Scheduler for AppHandle {
    fn submit(&self, task: UiTask) -> AppResult<()> {
        self.run_on_main_thread(task).map_err(AppError::from)
    }
}

pub(super) async fn run<S, T, F>(scheduler: S, work: F) -> AppResult<T>
where
    S: Scheduler,
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    let (sender, receiver) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn(async move {
        scheduler.submit(Box::new(move || {
            let _ = sender.send(work());
        }))
    })
    .await
    .map_err(|_| AppError::Internal("overlay UI dispatch task failed".into()))??;
    receiver
        .await
        .map_err(|_| AppError::Internal("overlay UI dispatch was closed".into()))?
}

/// Read the source state when the queued UI task executes, not at submission.
pub(super) async fn run_latest<S, V, T, R, F>(scheduler: S, read: R, work: F) -> AppResult<T>
where
    S: Scheduler,
    T: Send + 'static,
    R: FnOnce() -> V + Send + 'static,
    F: FnOnce(V) -> AppResult<T> + Send + 'static,
{
    run(scheduler, move || work(read())).await
}

#[cfg(test)]
mod tests;
