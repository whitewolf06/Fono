//! Optional signed public updates; installation requires an explicit UI action.
pub mod activity;
mod configuration;
#[cfg(windows)]
mod runtime;
mod state;
pub use state::{UpdatePhase, UpdateService, UpdateSnapshot};

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub fn status(app: &AppHandle) -> UpdateSnapshot {
    app.state::<UpdateService>()
        .snapshot(app.state::<AppState>().settings().update_checks_enabled)
}

pub async fn check(app: &AppHandle) -> AppResult<UpdateSnapshot> {
    #[cfg(windows)]
    {
        app.state::<UpdateService>().check(app).await?;
    }
    Ok(status(app))
}

pub async fn install(app: &AppHandle) -> AppResult<UpdateSnapshot> {
    #[cfg(windows)]
    {
        app.state::<UpdateService>().install(app).await?;
    }
    #[cfg(not(windows))]
    {
        return Err(AppError::Config(
            "Обновление доступно только в Windows-сборке.".into(),
        ));
    }
    #[cfg(windows)]
    Ok(status(app))
}

pub fn cancel(app: &AppHandle) -> UpdateSnapshot {
    app.state::<UpdateService>().cancel();
    status(app)
}

pub async fn set_checks_enabled(app: &AppHandle, enabled: bool) -> AppResult<UpdateSnapshot> {
    let _activity = activity::lease()?;
    let _transaction = settings_transaction(app).await;
    let service = app.state::<UpdateService>();
    if service.data.lock().snapshot.phase == UpdatePhase::Installing {
        return Err(AppError::Busy(
            "Установка обновления уже начинается.".into(),
        ));
    }
    let state = app.state::<AppState>();
    let base = state.settings();
    let mut settings = base.clone();
    settings.update_checks_enabled = enabled;
    let settings = state
        .persist_settings_delta(&base, &settings)
        .map_err(|_| {
            AppError::Config("Не удалось сохранить настройку проверки обновлений.".into())
        })?;
    crate::events::emit_settings(app, &settings);
    if !enabled {
        service.cancel();
    }
    Ok(status(app))
}

/// Serialize native settings read/modify/write operations, including narrow
/// feature toggles that may otherwise be overwritten by a stale full form.
pub(crate) async fn settings_transaction(app: &AppHandle) -> tokio::sync::OwnedMutexGuard<()> {
    Arc::clone(&app.state::<UpdateService>().settings_transaction)
        .lock_owned()
        .await
}

pub fn check_on_startup(app: AppHandle) {
    if !app.state::<AppState>().settings().update_checks_enabled {
        return;
    }
    tauri::async_runtime::spawn(async move {
        // One best-effort check per launch. Never starts download or installer.
        let _ = check(&app).await;
    });
}
