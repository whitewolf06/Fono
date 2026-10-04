//! Isolated settings previews never insert, archive or publish dictation text.
use crate::{
    error::{AppError, AppResult},
    llm::{presets, LlmClient},
    state::AppState,
    types::{
        ProcessingPromptChoice, Settings, TextPreset, TranslationLanguage,
        MAX_PROCESSING_PROMPT_CHARS, MAX_PROCESSING_TEXT_BYTES,
    },
};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
pub struct ProcessingPromptCatalog {
    presets: Vec<presets::ProcessingPromptDescriptor>,
    max_prompt_chars: usize,
    max_text_bytes: usize,
}

#[tauri::command]
pub fn get_processing_prompt_catalog() -> ProcessingPromptCatalog {
    ProcessingPromptCatalog {
        presets: presets::prompt_catalog(),
        max_prompt_chars: MAX_PROCESSING_PROMPT_CHARS,
        max_text_bytes: MAX_PROCESSING_TEXT_BYTES,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessingPreviewInput {
    text: String,
    preset: TextPreset,
    #[serde(default)]
    target_language: Option<TranslationLanguage>,
    #[serde(default)]
    prompt_override: Option<ProcessingPromptChoice>,
}

#[derive(Serialize)]
pub struct ProcessingPreviewResult {
    text: String,
    model: Option<String>,
    elapsed_ms: u64,
    preset: TextPreset,
    target_language: Option<TranslationLanguage>,
}

impl ProcessingPreviewInput {
    fn validate(&self) -> AppResult<()> {
        if self.text.trim().is_empty() || self.text.len() > MAX_PROCESSING_TEXT_BYTES {
            return Err(AppError::Config(format!(
                "Введите текст длиной до {MAX_PROCESSING_TEXT_BYTES} байт"
            )));
        }
        if self.preset == TextPreset::Raw && self.prompt_override.is_some() {
            return Err(AppError::Config(
                "У режима «Без изменений» нет системного промпта обработки".into(),
            ));
        }
        if let Some(choice) = &self.prompt_override {
            choice.validate()?;
        }
        Ok(())
    }

    fn choice<'a>(&'a self, settings: &'a Settings) -> Option<&'a ProcessingPromptChoice> {
        self.prompt_override
            .as_ref()
            .or_else(|| settings.processing_prompts.choice(self.preset))
    }
}

#[tauri::command]
pub async fn preview_processing_text(
    state: State<'_, AppState>,
    input: ProcessingPreviewInput,
) -> AppResult<ProcessingPreviewResult> {
    input.validate()?;
    let _activity = crate::application::updates::activity::lease()?;
    let started = std::time::Instant::now();
    let settings = state.settings();
    let (text, model) = if input.preset == TextPreset::Raw && input.target_language.is_none() {
        (input.text.clone(), None)
    } else {
        let profile = settings.correction_profile().ok_or_else(|| {
            AppError::Config(
                "Выберите подключение для обработки текста и сохраните настройки подключения"
                    .into(),
            )
        })?;
        let client =
            LlmClient::from_profile(profile, settings.text_correction_llm.model.as_deref());
        let result = client
            .process_preset(
                &input.text,
                input.preset,
                input.target_language,
                input.choice(&settings),
            )
            .await?;
        (result, client.selected_model().map(str::to_owned))
    };
    Ok(ProcessingPreviewResult {
        text,
        model,
        preset: input.preset,
        target_language: input.target_language,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

#[cfg(test)]
mod tests;
