//! Trainer subscriptions and periodic retention are wired by the composition root.

use crate::{
    history::{HistoryChangeKind, HistoryChanged, HistoryHooks},
    state::AppState,
};
use std::sync::Arc;
use tauri::Manager;

pub(crate) fn hooks() -> HistoryHooks {
    HistoryHooks {
        before_private_write: Arc::new(super::trainer_report::invalidate_private_cache_at),
        sources_changed: Arc::new(|change: HistoryChanged| match change.kind {
            HistoryChangeKind::Privacy => super::trainer_ai::invalidate_all_sessions(),
            HistoryChangeKind::Session => {
                for id in change.ids {
                    super::trainer_ai::invalidate_session(&id);
                }
            }
            HistoryChangeKind::Sources => super::trainer_ai::sources_changed(),
            _ => {}
        }),
    }
}

pub(crate) fn start_maintenance(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
            let Ok(lease) = super::updates::activity::lease() else {
                continue;
            };
            let transaction = super::updates::settings_transaction(&app).await;
            let settings = app.state::<AppState>().settings();
            let repository = crate::history::for_app(&app);
            let result = tauri::async_runtime::spawn_blocking(move || {
                let _lease = lease;
                let _transaction = transaction;
                repository.maintain(&settings)
            })
            .await;
            if !matches!(result, Ok(Ok(()))) {
                tracing::warn!("history retention maintenance failed");
            }
        }
    });
}
