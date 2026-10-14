//! Own complete position changes in request order, including persistence.
//! No state/geometry lock is held while waiting for the UI dispatcher.
use std::{future::Future, pin::Pin};

use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};

use crate::error::{AppError, AppResult};

type Job = Pin<Box<dyn Future<Output = ()> + Send>>;

#[derive(Default)]
pub(super) struct Queue {
    sender: Mutex<Option<mpsc::UnboundedSender<Job>>>,
}

impl Queue {
    pub async fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = AppResult<T>> + Send + 'static,
    ) -> AppResult<T> {
        let (reply, result) = oneshot::channel();
        {
            let mut sender = self.sender.lock();
            let sender = sender.get_or_insert_with(|| {
                let (sender, mut receiver) = mpsc::unbounded_channel::<Job>();
                tauri::async_runtime::spawn(async move {
                    while let Some(job) = receiver.recv().await {
                        job.await;
                    }
                });
                sender
            });
            sender
                .send(Box::pin(async move {
                    // Dropping the command's receiver never cancels this work.
                    let outcome = work.await;
                    let _ = reply.send(outcome);
                }))
                .map_err(|_| AppError::Internal("overlay position queue was closed".into()))?;
        }
        result
            .await
            .map_err(|_| AppError::Internal("overlay position task was closed".into()))?
    }
}
