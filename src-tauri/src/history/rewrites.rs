//! Archive editor contracts; variants never replace the session's source/result.
use super::HistoryRepository;
use crate::{
    error::{AppError, AppResult},
    types::DictationHistoryEntry,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;

pub const MAX_TEXT_BYTES: usize = 128 * 1024;
pub const PROMPT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteSource {
    Original,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteStyle {
    Preserve,
    Concise,
    Restrained,
    Friendly,
    Lively,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewriteVariant {
    pub id: String,
    pub entry_id: String,
    pub source: RewriteSource,
    pub source_fingerprint: String,
    pub style: RewriteStyle,
    pub profile_id: String,
    pub profile_name: String,
    pub profile_revision: String,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: RewriteStatus,
    pub text: String,
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteStatus {
    Ready,
}

pub(crate) struct RewriteMaterial {
    pub text: String,
    pub fingerprint: String,
    pub epoch: u64,
}

pub(crate) fn fingerprint(value: &impl Serialize) -> AppResult<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

pub(crate) fn material(
    entry: &DictationHistoryEntry,
    source: RewriteSource,
) -> AppResult<(&str, String)> {
    let text = match source {
        RewriteSource::Original => entry.original_text.as_deref(),
        RewriteSource::Result => Some(entry.text.as_str()),
    }
    .filter(|text| !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES && !text.contains('\0'))
    .ok_or_else(unavailable)?;
    Ok((
        text,
        fingerprint(&(&entry.id, entry.created_at, source, text))?,
    ))
}

pub(crate) fn cache_key(
    entry: &str,
    source: &str,
    style: RewriteStyle,
    profile: &str,
) -> AppResult<String> {
    fingerprint(&(PROMPT_VERSION, entry, source, style, profile))
}

pub(crate) fn unavailable() -> AppError {
    AppError::Config("Запись или её исходный текст уже недоступны. Откройте историю заново.".into())
}

impl HistoryRepository {
    pub(crate) fn rewrite_epoch(&self) -> u64 {
        self.rewrite_epoch.load(Ordering::SeqCst)
    }
    pub(crate) fn rewrite_material(
        &self,
        id: &str,
        source: RewriteSource,
    ) -> AppResult<RewriteMaterial> {
        let store = self.read_access()?;
        let entry = store
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(unavailable)?;
        let (text, fingerprint) = material(entry, source)?;
        Ok(RewriteMaterial {
            text: text.to_owned(),
            fingerprint,
            epoch: self.rewrite_epoch.load(Ordering::SeqCst),
        })
    }

    pub(crate) fn rewrite_variants(&self, id: &str) -> AppResult<Vec<RewriteVariant>> {
        let store = self.read_access()?;
        if !store.entries.iter().any(|entry| entry.id == id) {
            return Err(unavailable());
        }
        store.rewrite_variants(id)
    }

    #[cfg(test)]
    pub(crate) fn save_rewrite(
        &self,
        variant: &RewriteVariant,
        epoch: u64,
    ) -> AppResult<RewriteVariant> {
        self.save_rewrite_guarded(variant, epoch, || true)
    }

    pub(crate) fn save_rewrite_guarded(
        &self,
        variant: &RewriteVariant,
        epoch: u64,
        current: impl Fn() -> bool,
    ) -> AppResult<RewriteVariant> {
        let mut store = self.read_access()?;
        if epoch != self.rewrite_epoch.load(Ordering::SeqCst) {
            return Err(unavailable());
        }
        let entry = store
            .entries
            .iter()
            .find(|entry| entry.id == variant.entry_id)
            .ok_or_else(unavailable)?;
        if material(entry, variant.source)?.1 != variant.source_fingerprint {
            return Err(unavailable());
        }
        if variant.text.trim().is_empty()
            || variant.text.len() > MAX_TEXT_BYTES
            || variant.text.contains('\0')
        {
            return Err(AppError::Llm(
                "Не удалось подготовить вариант. Исходный текст сохранён.".into(),
            ));
        }
        store.save_rewrite(variant, || {
            current()
                && epoch == self.rewrite_epoch.load(Ordering::SeqCst)
                && !self.privacy_blocked.load(Ordering::SeqCst)
        })
    }

    pub(crate) fn delete_rewrite(&self, entry_id: &str, variant_id: &str) -> AppResult<()> {
        let mut store = self.read_access()?;
        // Revoke an in-flight completion before deleting any matching cache row.
        self.rewrite_epoch.fetch_add(1, Ordering::SeqCst);
        store.delete_rewrite(entry_id, variant_id)
    }
}

#[cfg(test)]
mod tests;
