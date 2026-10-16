//! Offload native I/O without releasing update admission when an IPC waiter goes away.
use crate::error::{AppError, AppResult};

pub(super) async fn run<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let activity = crate::application::updates::activity::lease()?;
    run_admitted(activity, work).await
}

pub(super) async fn run_admitted<T: Send + 'static>(
    activity: crate::application::updates::activity::ActivityLease<'static>,
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    run_with_guard(activity, work).await
}

async fn run_with_guard<T: Send + 'static, G: Send + 'static>(
    guard: G,
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        work()
    })
    .await
    .map_err(|_| AppError::Internal("Фоновая операция не завершилась".into()))?
}

#[cfg(test)]
mod tests {
    use super::run_with_guard;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn blocking_io_keeps_runtime_responsive_and_owns_guard_after_waiter_cancellation() {
        let released = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let guard = Guard(released.clone());
        let caller_thread = std::thread::current().id();
        let waiter = tokio::spawn(run_with_guard(guard, move || {
            assert_ne!(std::thread::current().id(), caller_thread);
            let _ = started_tx.send(());
            finish_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            Ok(())
        }));
        // The default test runtime has only this async thread. Receiving the
        // start notification requires the native work to run on another thread.
        tokio::time::timeout(std::time::Duration::from_secs(5), started_rx)
            .await
            .unwrap()
            .unwrap();
        waiter.abort();
        let _ = waiter.await;
        assert!(!released.load(Ordering::Acquire));
        finish_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !released.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
