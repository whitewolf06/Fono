//! Explicit archive editing owns its work and updater lease beyond IPC waiters.
mod profiles;
use crate::{
    error::{AppError, AppResult},
    history::{
        rewrites::{self, RewriteSource, RewriteStatus, RewriteStyle, RewriteVariant},
        HistoryRepository,
    },
    state::AppState,
    types::Settings,
};
use fono_core::OperationCancellation;
use parking_lot::Mutex;
pub(crate) use profiles::{profile_options, RewriteProfile};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub(crate) struct Runtime {
    job: Arc<Mutex<Option<Job>>>,
}
struct Job {
    id: String,
    cancel: OperationCancellation,
}
struct JobGuard {
    slot: Arc<Mutex<Option<Job>>>,
    id: String,
}
impl Drop for JobGuard {
    fn drop(&mut self) {
        let mut slot = self.slot.lock();
        if slot.as_ref().is_some_and(|job| job.id == self.id) {
            slot.take();
        }
    }
}
impl Runtime {
    fn begin(&self, id: &str) -> AppResult<(JobGuard, OperationCancellation)> {
        if id.is_empty()
            || id.len() > 96
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            return Err(rewrites::unavailable());
        }
        let mut slot = self.job.lock();
        if slot.is_some() {
            return Err(AppError::Busy(
                "Дождитесь завершения текущего варианта.".into(),
            ));
        }
        let cancel = OperationCancellation::default();
        *slot = Some(Job {
            id: id.to_owned(),
            cancel: cancel.clone(),
        });
        Ok((
            JobGuard {
                slot: self.job.clone(),
                id: id.to_owned(),
            },
            cancel,
        ))
    }
    pub(crate) fn cancel(&self, id: &str) {
        if let Some(job) = self.job.lock().as_ref().filter(|job| job.id == id) {
            job.cancel.cancel();
        }
    }
    pub(crate) fn shutdown(&self) {
        if let Some(job) = self.job.lock().as_ref() {
            job.cancel.cancel();
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RewriteRequest {
    pub operation_id: String,
    pub entry_id: String,
    pub source: RewriteSource,
    pub source_fingerprint: String,
    pub style: RewriteStyle,
    pub profile_id: String,
    pub profile_revision: String,
    pub cloud_consent: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RewriteContext {
    pub enabled: bool,
    pub profiles: Vec<RewriteProfile>,
    pub default_profile_id: Option<String>,
    pub original_fingerprint: Option<String>,
    pub result_fingerprint: Option<String>,
    pub external_delivery: &'static str,
    pub variants: Vec<RewriteVariant>,
}

pub(crate) fn context(
    repository: &HistoryRepository,
    settings: &Settings,
    entry: &str,
) -> AppResult<RewriteContext> {
    let original_fingerprint = repository
        .rewrite_material(entry, RewriteSource::Original)
        .ok()
        .map(|m| m.fingerprint);
    let result_fingerprint = repository
        .rewrite_material(entry, RewriteSource::Result)
        .ok()
        .map(|m| m.fingerprint);
    let profiles = profile_options(settings)?;
    let mut variants = repository.rewrite_variants(entry)?;
    for variant in &mut variants {
        let source = match variant.source {
            RewriteSource::Original => &original_fingerprint,
            RewriteSource::Result => &result_fingerprint,
        };
        variant.stale = source.as_ref() != Some(&variant.source_fingerprint)
            || !profiles.iter().any(|profile| {
                profile.id == variant.profile_id && profile.revision == variant.profile_revision
            });
    }
    let default_profile_id = settings
        .text_correction_llm
        .profile_id
        .clone()
        .filter(|id| profiles.iter().any(|p| p.id == *id));
    Ok(RewriteContext {
        enabled: settings.history_enabled,
        profiles,
        default_profile_id,
        original_fingerprint,
        result_fingerprint,
        external_delivery: "unknown",
        variants,
    })
}

pub(crate) async fn run(app: AppHandle, request: RewriteRequest) -> AppResult<RewriteVariant> {
    let lease = super::updates::activity::lease()?;
    let (guard, cancel) = app.state::<Runtime>().begin(&request.operation_id)?;
    // The spawned task retains both admission and job ownership when its IPC waiter disappears.
    tauri::async_runtime::spawn(async move {
        let _lease = lease;
        let _guard = guard;
        execute(&app, request, cancel).await
    })
    .await
    .map_err(|_| AppError::Internal("Фоновая операция не завершилась".into()))?
}

fn cancelled() -> AppError {
    AppError::Cancelled("Редактура отменена. Исходный текст сохранён.".into())
}

async fn execute(
    app: &AppHandle,
    request: RewriteRequest,
    cancel: OperationCancellation,
) -> AppResult<RewriteVariant> {
    let owner = app.clone();
    execute_with(
        crate::history::for_app(app),
        move || owner.state::<AppState>().settings(),
        request,
        cancel,
    )
    .await
}

async fn execute_with(
    repository: Arc<HistoryRepository>,
    settings_now: impl Fn() -> Settings,
    request: RewriteRequest,
    cancel: OperationCancellation,
) -> AppResult<RewriteVariant> {
    let settings = settings_now();
    if !settings.history_enabled {
        return Err(rewrites::unavailable());
    }
    let profile = profiles::selected(&settings, &request.profile_id, &request.profile_revision)?;
    if profile.cloud && !request.cloud_consent {
        return Err(AppError::Config(
            "Подтвердите передачу текста выбранному облачному подключению.".into(),
        ));
    }
    let native_profile = settings
        .llm_profile(Some(&profile.id))
        .cloned()
        .ok_or_else(rewrites::unavailable)?;
    let prep_repository = repository.clone();
    let entry_id = request.entry_id.clone();
    let source = request.source;
    let expected = request.source_fingerprint.clone();
    let model = profile.model.clone();
    let client_lease = super::updates::activity::lease()?;
    let (material, client) = tauri::async_runtime::spawn_blocking(move || {
        let _lease = client_lease;
        let material = prep_repository.rewrite_material(&entry_id, source)?;
        if material.fingerprint != expected {
            return Err(rewrites::unavailable());
        }
        let client = crate::llm::LlmClient::from_profile(&native_profile, Some(&model));
        Ok((material, client))
    })
    .await
    .map_err(|_| AppError::Internal("Фоновая операция не завершилась".into()))??;
    if cancel.is_cancelled() || repository.rewrite_epoch() != material.epoch {
        return Err(cancelled());
    }
    if !client.is_configured(profile.requires_key) {
        return Err(AppError::Config(
            "Выберите подключение и модель для обработки текста".into(),
        ));
    }
    let key = rewrites::cache_key(
        &request.entry_id,
        &material.fingerprint,
        request.style,
        &profile.revision,
    )?;
    let cache_repository = repository.clone();
    let cache_entry = request.entry_id.clone();
    let cache_key = key.clone();
    let cached = blocking(move || {
        Ok(cache_repository
            .rewrite_variants(&cache_entry)?
            .into_iter()
            .find(|v| v.id == cache_key))
    })
    .await?;
    if cancel.is_cancelled() || repository.rewrite_epoch() != material.epoch {
        return Err(cancelled());
    }
    // Validate again before transport; selection/consent refers to the displayed profile snapshot.
    profiles::selected(&settings_now(), &profile.id, &profile.revision)?;
    if let Some(cached) = cached {
        return Ok(cached);
    }
    let text = tokio::select! {
        biased;
        _ = wait_cancelled(cancel.clone(), repository.clone(), material.epoch) => return Err(cancelled()),
        result = client.rewrite_history(&material.text, request.style) => result?,
    };
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    profiles::selected(&settings_now(), &profile.id, &profile.revision)?;
    let variant = RewriteVariant {
        id: key,
        entry_id: request.entry_id,
        source: request.source,
        source_fingerprint: material.fingerprint,
        style: request.style,
        profile_id: profile.id,
        profile_name: profile.name,
        profile_revision: profile.revision,
        model: profile.model,
        created_at: chrono::Utc::now(),
        status: RewriteStatus::Ready,
        text,
        stale: false,
    };
    blocking(move || {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        repository.save_rewrite_guarded(&variant, material.epoch, || !cancel.is_cancelled())
    })
    .await
}

async fn wait_cancelled(
    cancel: OperationCancellation,
    repository: Arc<HistoryRepository>,
    epoch: u64,
) {
    while !cancel.is_cancelled() && repository.rewrite_epoch() == epoch {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let lease = super::updates::activity::lease()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        work()
    })
    .await
    .map_err(|_| AppError::Internal("Фоновая операция не завершилась".into()))?
}

#[cfg(test)]
mod tests;
