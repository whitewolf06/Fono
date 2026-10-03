//! Read-only command; collection never opens a microphone or sends a report.
use tauri::AppHandle;

#[tauri::command]
pub async fn get_diagnostic_report(
    app: AppHandle,
) -> Result<crate::application::diagnostic_report::DiagnosticReport, &'static str> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::application::diagnostic_report::collect(&app)
    })
    .await
    .map_err(|_| "Не удалось создать отчёт диагностики")
}
