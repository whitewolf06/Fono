use crate::{
    application::{
        file_transcription::{self, SubmitFilesResult},
        service_control::ServiceControl,
        transcript_export::{self, ExportFormat},
    },
    error::{AppError, AppResult},
};
use tauri::{AppHandle, Manager, WebviewWindow};

fn require_main(window: &WebviewWindow) -> AppResult<()> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Работа с файлами доступна в основном окне Fono.".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn choose_transcription_files(
    app: AppHandle,
    window: WebviewWindow,
) -> AppResult<SubmitFilesResult> {
    require_main(&window)?;
    let paths = file_transcription::dialog(app.clone(), None).await?;
    submit_transcription_files(app, window, paths).await
}

#[tauri::command]
pub async fn submit_transcription_files(
    app: AppHandle,
    window: WebviewWindow,
    paths: Vec<std::path::PathBuf>,
) -> AppResult<SubmitFilesResult> {
    require_main(&window)?;
    super::blocking::run(move || app.state::<ServiceControl>().submit_files(&app, paths)).await
}

#[tauri::command]
pub async fn export_transcription_file(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
    format: ExportFormat,
) -> AppResult<bool> {
    require_main(&window)?;
    let source_app = app.clone();
    let content = super::blocking::run(move || {
        let job = source_app
            .state::<ServiceControl>()
            .get_job(&id)
            .or_else(|| {
                crate::service_history::snapshot()
                    .ok()?
                    .jobs
                    .into_iter()
                    .find(|job| job.id == id)
            })
            .ok_or_else(|| AppError::Config("Результат уже удалён из истории.".into()))?;
        let result = job
            .result
            .ok_or_else(|| AppError::Config("Дождитесь окончания распознавания.".into()))?;
        transcript_export::render(&result, format)
    })
    .await?;
    let paths = file_transcription::dialog(app, Some(format)).await?;
    let Some(path) = paths.into_iter().next() else {
        return Ok(false);
    };
    super::blocking::run(move || {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension()))
        {
            return Err(AppError::Config(format!(
                "Укажите расширение .{} для выбранного формата.",
                format.extension()
            )));
        }
        std::fs::write(path, content).map_err(|_| {
            AppError::Config("Не удалось сохранить файл. Проверьте папку и права доступа.".into())
        })?;
        Ok(true)
    })
    .await
}
