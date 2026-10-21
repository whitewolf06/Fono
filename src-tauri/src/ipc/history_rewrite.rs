use crate::{
    application::history_rewrite::{self, RewriteContext, RewriteRequest},
    error::AppResult,
    history::rewrites::RewriteVariant,
    state::AppState,
};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) async fn get_history_rewrite_context(
    app: AppHandle,
    entry_id: String,
) -> AppResult<RewriteContext> {
    let settings = app.state::<AppState>().settings();
    super::blocking::run(move || {
        history_rewrite::context(&crate::history::for_app(&app), &settings, &entry_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_history_rewrite(
    app: AppHandle,
    request: RewriteRequest,
) -> AppResult<RewriteVariant> {
    history_rewrite::run(app, request).await
}

#[tauri::command]
pub(crate) fn cancel_history_rewrite(app: AppHandle, operation_id: String) {
    app.state::<history_rewrite::Runtime>()
        .cancel(&operation_id);
}

#[tauri::command]
pub(crate) async fn delete_history_rewrite(
    app: AppHandle,
    entry_id: String,
    variant_id: String,
) -> AppResult<()> {
    super::blocking::run(move || {
        crate::history::for_app(&app).delete_rewrite(&entry_id, &variant_id)
    })
    .await
}
