//! Per-preset editor instructions. Legacy clean instructions migrate once.
use crate::{
    error::{AppError, AppResult},
    types::{Settings, TextPreset},
};
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_PROCESSING_PROMPT_CHARS: usize = 12_000;
pub const MAX_PROCESSING_TEXT_BYTES: usize = 200_000;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessingPromptChoice {
    #[serde(default)]
    pub use_custom: bool,
    #[serde(default)]
    pub custom_prompt: String,
}

impl ProcessingPromptChoice {
    pub fn validate(&self) -> AppResult<()> {
        if self.custom_prompt.chars().count() > MAX_PROCESSING_PROMPT_CHARS {
            return Err(AppError::Config(format!(
                "Системный промпт должен содержать не более {MAX_PROCESSING_PROMPT_CHARS} символов"
            )));
        }
        if self.use_custom && self.custom_prompt.trim().is_empty() {
            return Err(AppError::Config(
                "Введите свой системный промпт или выберите стандартный".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ProcessingPrompts {
    pub clean: ProcessingPromptChoice,
    pub format: ProcessingPromptChoice,
    pub task: ProcessingPromptChoice,
    pub formal: ProcessingPromptChoice,
    // Distinguish absent legacy settings from an explicit standard choice.
    #[serde(skip)]
    clean_configured: bool,
}

impl PartialEq for ProcessingPrompts {
    fn eq(&self, other: &Self) -> bool {
        self.clean == other.clean
            && self.format == other.format
            && self.task == other.task
            && self.formal == other.formal
    }
}
impl Eq for ProcessingPrompts {}

impl<'de> Deserialize<'de> for ProcessingPrompts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            #[serde(default)]
            clean: Option<ProcessingPromptChoice>,
            #[serde(default)]
            format: ProcessingPromptChoice,
            #[serde(default)]
            task: ProcessingPromptChoice,
            #[serde(default)]
            formal: ProcessingPromptChoice,
        }
        let fields = Fields::deserialize(deserializer)?;
        Ok(Self {
            clean_configured: fields.clean.is_some(),
            clean: fields.clean.unwrap_or_default(),
            format: fields.format,
            task: fields.task,
            formal: fields.formal,
        })
    }
}

impl ProcessingPrompts {
    pub fn choice(&self, preset: TextPreset) -> Option<&ProcessingPromptChoice> {
        match preset {
            TextPreset::Raw => None,
            TextPreset::Clean => Some(&self.clean),
            TextPreset::Format => Some(&self.format),
            TextPreset::Task => Some(&self.task),
            TextPreset::Formal => Some(&self.formal),
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        for choice in [&self.clean, &self.format, &self.task, &self.formal] {
            choice.validate()?;
        }
        Ok(())
    }
}

impl Settings {
    /// Preserve a previous clean prompt, but never resurrect it after choosing
    /// standard instructions in the new UI. Mirror clean for older renderers.
    pub fn migrate_processing_prompts(&mut self) -> bool {
        let before = self.processing_prompts.clone();
        let old_legacy = self.clean_prompt.clone();
        if !self.processing_prompts.clean_configured {
            if let Some(prompt) = self.clean_prompt.as_ref().filter(|p| !p.trim().is_empty()) {
                self.processing_prompts.clean = ProcessingPromptChoice {
                    use_custom: true,
                    custom_prompt: prompt.clone(),
                };
            }
            self.processing_prompts.clean_configured = true;
        }
        self.clean_prompt = self
            .processing_prompts
            .clean
            .use_custom
            .then(|| self.processing_prompts.clean.custom_prompt.clone());
        before.clean != self.processing_prompts.clean || old_legacy != self.clean_prompt
    }
}

#[cfg(test)]
mod tests;
