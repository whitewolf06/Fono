use crate::{
    application::{
        file_transcription,
        snippets::{self, Snippet},
    },
    error::{AppError, AppResult},
};
use tauri::{AppHandle, WebviewWindow};

fn require_main(window: &WebviewWindow) -> AppResult<()> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Сниппеты доступны в основном окне Fono.".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn import_snippets(
    app: AppHandle,
    window: WebviewWindow,
) -> AppResult<Option<Vec<Snippet>>> {
    require_main(&window)?;
    let paths = file_transcription::json_dialog(app, false).await?;
    let Some(path) = paths.into_iter().next() else {
        return Ok(None);
    };
    // Validation returns a draft. Persisting it requires the existing Settings save.
    super::blocking::run(move || snippets::read_json_file(&path).map(Some)).await
}

#[tauri::command]
pub async fn export_snippets(
    app: AppHandle,
    window: WebviewWindow,
    entries: Vec<Snippet>,
) -> AppResult<bool> {
    require_main(&window)?;
    let content = snippets::to_json(&entries)?;
    let paths = file_transcription::json_dialog(app, true).await?;
    let Some(path) = paths.into_iter().next() else {
        return Ok(false);
    };
    super::blocking::run(move || {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            return Err(AppError::Config("Укажите расширение .json.".into()));
        }
        std::fs::write(path, content).map_err(|_| {
            AppError::Config(
                "Не удалось сохранить JSON сниппетов. Проверьте папку и доступ.".into(),
            )
        })?;
        Ok(true)
    })
    .await
}
