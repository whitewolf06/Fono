//! Thin async UI facade; every network/credential action owns blocking admission.
use crate::{
    application::remote_stt::{self, CredentialStatus, TestRequest, TestResult},
    error::AppResult,
};
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

#[tauri::command]
pub(crate) async fn get_remote_stt_status() -> AppResult<CredentialStatus> {
    super::blocking::run(remote_stt::credential_status).await
}

#[tauri::command]
pub(crate) async fn test_remote_stt_server(
    app: AppHandle,
    request: TestRequest,
) -> AppResult<TestResult> {
    inspect(app, Some(request)).await
}

#[tauri::command]
pub(crate) async fn list_remote_stt_models(app: AppHandle) -> AppResult<TestResult> {
    inspect(app, None).await
}

async fn inspect(app: AppHandle, request: Option<TestRequest>) -> AppResult<TestResult> {
    let activity = crate::application::updates::activity::lease()?;
    let transaction = crate::application::updates::settings_transaction(&app).await;
    super::blocking::run_admitted(activity, move || {
        // Matching the public saved endpoint and reading its credential must
        // observe one settings/secret transaction. The blocking task retains
        // both guards even when the renderer drops its IPC waiter.
        let _transaction = transaction;
        let settings = app.state::<crate::state::AppState>().settings();
        if let Some(request) = request {
            return Ok(remote_stt::test(request, settings.remote_stt.as_ref()));
        }
        Ok(match settings.remote_stt.as_ref() {
            Some(remote) => remote_stt::test(remote_stt::saved_request(remote), Some(remote)),
            None => TestResult::Error {
                error: remote_stt::ConnectionError::NotConfigured,
            },
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_remote_stt_token(app: AppHandle, token: String) -> AppResult<()> {
    change(app, Some(Zeroizing::new(token))).await
}

#[tauri::command]
pub(crate) async fn clear_remote_stt_token(app: AppHandle) -> AppResult<()> {
    change(app, None).await
}

async fn change(app: AppHandle, token: Option<Zeroizing<String>>) -> AppResult<()> {
    let activity = crate::application::updates::activity::lease()?;
    let transaction = crate::application::updates::settings_transaction(&app).await;
    super::blocking::run_admitted(activity, move || {
        let _transaction = transaction;
        remote_stt::change_token(&app, token.as_deref().map(String::as_str))
    })
    .await
}
