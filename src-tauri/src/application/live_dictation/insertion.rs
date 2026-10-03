//! Every fragment is acknowledged once. Uncertain OS writes are never retried.
use super::session::LiveSession;
use crate::{injection::target, pipeline::Pipeline};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub(super) async fn drain(app: &AppHandle, session: &Arc<LiveSession>) {
    drain_chunks(
        || {
            if session.cancellation.is_cancelled()
                || !app
                    .state::<Pipeline>()
                    .is_operation_active(session.operation)
            {
                return None;
            }
            let data = session.data.lock();
            (data.snapshot.insertion_state == "active").then_some(data.snapshot.pending_text.len())
        },
        || flush(app, session),
    )
    .await;
}

async fn drain_chunks<S, F, R>(mut pending: S, mut flush: F)
where
    S: FnMut() -> Option<usize>,
    F: FnMut() -> R,
    R: std::future::Future<Output = ()>,
{
    while let Some(before) = pending().filter(|length| *length > 0) {
        flush().await;
        // Never retry paused/failed/cancelled or unacknowledged writes.
        if !pending().is_some_and(|after| after < before) {
            break;
        }
    }
}

pub(super) async fn observe_focus(app: AppHandle, session: Arc<LiveSession>) {
    while !session.finished.load(std::sync::atomic::Ordering::Acquire)
        && !session.cancellation.is_cancelled()
        && super::owns(&app, &session)
    {
        let target = {
            let data = session.data.lock();
            (data.snapshot.insertion_state == "active")
                .then(|| data.target.clone())
                .flatten()
        };
        if let Some(target) = target {
            let probe = target.clone();
            let same = tauri::async_runtime::spawn_blocking(move || target::matches(&probe))
                .await
                .unwrap_or(false);
            if !same && !session.finished.load(std::sync::atomic::Ordering::Acquire) {
                let mut data = session.data.lock();
                if data.target.as_ref() == Some(&target)
                    && data.snapshot.insertion_state == "active"
                {
                    data.snapshot.insertion_state = "paused_focus".into();
                    data.snapshot.warning = Some("Поле ввода изменилось. Распознавание продолжается; возобновите вставку явно".into());
                }
                drop(data);
                session.emit(&app);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

pub(super) async fn flush(app: &AppHandle, session: &Arc<LiveSession>) {
    let _serial = session.insertion.lock().await;
    let request = {
        let data = session.data.lock();
        if data.snapshot.insertion_state != "active" {
            return;
        }
        data.target.clone().map(|target| {
            (
                target,
                data.snapshot
                    .pending_text
                    .chars()
                    .take(128)
                    .collect::<String>(),
            )
        })
    };
    let Some((target, text)) = request else {
        return;
    };
    if session.cancellation.is_cancelled()
        || !app
            .state::<Pipeline>()
            .is_operation_active(session.operation)
    {
        return;
    }
    let cancel = session.cancellation.clone();
    let mode = session.settings.injection_mode;
    let result = tauri::async_runtime::spawn_blocking(move || {
        if text.is_empty() {
            Ok(target::AppendResult {
                bytes: 0,
                paused: !target::matches(&target),
                uncertain: false,
            })
        } else {
            target::append(&target, &text, mode, &cancel)
        }
    })
    .await;
    let mut data = session.data.lock();
    match result {
        Ok(Ok(ack)) => {
            let bytes = ack.bytes.min(data.snapshot.pending_text.len());
            if data.snapshot.pending_text.is_char_boundary(bytes) {
                data.snapshot.pending_text.drain(..bytes);
            }
            if ack.uncertain {
                data.snapshot.insertion_state = "failed".into();
                data.snapshot.warning = Some(
                    "Вставка подтверждена не полностью. Скопируйте оставшийся текст вручную".into(),
                );
            } else if ack.paused {
                data.snapshot.insertion_state = "paused_focus".into();
                data.snapshot.warning = Some("Поле ввода изменилось. Распознавание продолжается; выберите поле и возобновите вставку".into());
            }
        }
        Ok(Err(error)) => {
            data.snapshot.insertion_state = "failed".into();
            data.snapshot.warning = Some(error.to_string());
        }
        Err(error) => {
            data.snapshot.insertion_state = "failed".into();
            data.snapshot.warning = Some(format!("Не удалось подтвердить вставку: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::drain_chunks;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn final_drain_sends_all_bounded_unicode_chunks() {
        let queue = Arc::new(Mutex::new("ё".repeat(300)));
        let calls = Arc::new(Mutex::new(0));
        drain_chunks(
            || Some(queue.lock().unwrap().len()),
            || {
                let queue = queue.clone();
                let calls = calls.clone();
                async move {
                    let mut text = queue.lock().unwrap();
                    let bytes = text.chars().take(128).map(char::len_utf8).sum::<usize>();
                    text.drain(..bytes);
                    *calls.lock().unwrap() += 1;
                }
            },
        )
        .await;
        assert!(queue.lock().unwrap().is_empty());
        assert_eq!(*calls.lock().unwrap(), 3);
    }

    #[tokio::test]
    async fn final_drain_does_not_retry_an_unacknowledged_write() {
        let calls = Arc::new(Mutex::new(0));
        drain_chunks(
            || Some(500),
            || {
                let calls = calls.clone();
                async move {
                    *calls.lock().unwrap() += 1;
                }
            },
        )
        .await;
        assert_eq!(*calls.lock().unwrap(), 1);
    }
}
