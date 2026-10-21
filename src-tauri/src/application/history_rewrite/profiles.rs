use super::*;
use crate::types::{LlmConnectionKind, LlmProvider};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RewriteProfile {
    pub id: String,
    pub name: String,
    pub model: String,
    pub revision: String,
    pub cloud: bool,
    pub configured: bool,
    #[serde(skip)]
    pub requires_key: bool,
}

pub(crate) fn profile_options(settings: &Settings) -> AppResult<Vec<RewriteProfile>> {
    settings
        .llm_profiles
        .iter()
        .map(|profile| {
            let assignment = (settings.text_correction_llm.profile_id.as_deref()
                == Some(profile.id.as_str()))
            .then_some(settings.text_correction_llm.model.as_deref())
            .flatten();
            let model = assignment
                .filter(|v| !v.trim().is_empty())
                .or(profile.model.as_deref())
                .unwrap_or("")
                .trim()
                .to_owned();
            let cloud = profile.connection == LlmConnectionKind::Cloud
                || profile.provider == LlmProvider::OpenAi;
            let base_url =
                if profile.provider == LlmProvider::OpenAi && profile.base_url.trim().is_empty() {
                    "https://api.openai.com/v1"
                } else {
                    &profile.base_url
                };
            let endpoint_ready = reqwest::Url::parse(base_url).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
            });
            let revision = rewrites::fingerprint(&(
                rewrites::PROMPT_VERSION,
                &profile.id,
                profile.provider,
                profile.connection,
                base_url,
                &model,
            ))?;
            Ok(RewriteProfile {
                id: profile.id.clone(),
                name: profile.name.clone(),
                configured: !model.is_empty() && endpoint_ready && (!cloud || profile.has_api_key),
                model,
                revision,
                cloud,
                requires_key: cloud,
            })
        })
        .collect()
}

pub(super) fn selected(settings: &Settings, id: &str, revision: &str) -> AppResult<RewriteProfile> {
    profile_options(settings)?
        .into_iter()
        .find(|p| p.id == id && p.revision == revision)
        .ok_or_else(|| AppError::Config("Подключение изменилось. Выберите его заново.".into()))
}
