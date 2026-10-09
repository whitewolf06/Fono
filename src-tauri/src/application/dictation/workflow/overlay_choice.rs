//! Overlay selection is a delta transaction, never a stale full settings save.
use super::{state::Store, PendingPhase, Runtime, TextPreset, TranslationLanguage};
use crate::{
    application::{capture_configuration, updates},
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
    types::Settings,
};
use fono_core::OperationPhase;
use serde::{Deserialize, Deserializer};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayProcessingChoiceRequest {
    #[serde(deserialize_with = "required_nullable")]
    pub session_id: Option<u64>,
    pub preset: TextPreset,
    #[serde(deserialize_with = "required_nullable")]
    pub target_language: Option<TranslationLanguage>,
    pub processing_enabled: Option<bool>,
    pub translation_enabled: Option<bool>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

pub(crate) async fn update(
    app: AppHandle,
    request: OverlayProcessingChoiceRequest,
) -> AppResult<Settings> {
    // Acquire admission before pipeline/store locks. The lease also covers
    // waiting for another settings writer, so installation cannot overtake us.
    let _activity = updates::activity::lease()?;
    let _transaction = updates::settings_transaction(&app).await;
    let _configuration = capture_configuration::reserve_configuration(|| Ok(()))?;
    let pipeline = app.state::<Pipeline>();
    let state = app.state::<AppState>();
    let runtime = app.state::<Runtime>();
    let persisted = pipeline.with_processing_choice(request.session_id, |phase, session| {
        let mut store = runtime.store.lock();
        let pending = editable_pending(&mut store, request.session_id, phase)?;
        let base = state.settings();
        persist_choice(
            &request,
            &base,
            session,
            pending.map(|record| (&mut record.snapshot, &mut record.data.session.settings)),
            |base, candidate| state.persist_settings_delta(base, candidate),
        )
    })?;
    // Both snapshots and disk now agree. Publish only after releasing capture
    // and pending locks; clients can receive Stop/Cancel in either event order.
    crate::events::emit_settings(&app, &persisted);
    super::publish(&app);
    Ok(without_secrets(persisted))
}

fn editable_pending<T>(
    store: &mut Store<T>,
    id: Option<u64>,
    phase: Option<OperationPhase>,
) -> AppResult<Option<&mut super::state::Record<T>>> {
    match phase {
        Some(OperationPhase::AwaitingAction) => {
            let record = store
                .pending
                .as_mut()
                .filter(|record| Some(record.snapshot.session_id) == id)
                .ok_or_else(|| AppError::Cancelled("Ожидающая диктовка уже завершена".into()))?;
            if record.snapshot.phase != PendingPhase::AwaitingAction {
                return Err(AppError::Busy("Диктовка уже обрабатывается".into()));
            }
            Ok(Some(record))
        }
        _ if store.pending.is_none() => Ok(None),
        _ => Err(AppError::Busy(
            "Сначала завершите ожидающую диктовку".into(),
        )),
    }
}

fn apply_choice(settings: &mut Settings, request: &OverlayProcessingChoiceRequest) {
    settings.processing_preset = Some(request.preset);
    settings.processing_target_language = request.target_language;
    if let Some(enabled) = request.processing_enabled {
        settings.ai_mode = if enabled {
            crate::types::AiMode::Clean
        } else {
            crate::types::AiMode::Off
        };
    }
    if let Some(enabled) = request.translation_enabled {
        settings.processing_translation_enabled = enabled;
    }
}

fn persist_choice(
    request: &OverlayProcessingChoiceRequest,
    base: &Settings,
    session: Option<&mut Settings>,
    pending: Option<(&mut super::PendingDictation, &mut Settings)>,
    persist: impl FnOnce(&Settings, &Settings) -> AppResult<Settings>,
) -> AppResult<Settings> {
    let mut candidate = base.clone();
    apply_choice(&mut candidate, request);
    let persisted = persist(base, &candidate)?;
    // No fallible work remains. A failed disk save changes neither the capture
    // snapshot nor pending selection and its Session used by resolve().
    if let Some(session) = session {
        apply_choice(session, request);
    }
    if let Some((snapshot, settings)) = pending {
        snapshot.preset = request.preset;
        apply_choice(settings, request);
        snapshot.target_language = super::effective_language(settings);
        snapshot.processing_enabled = settings.ai_mode != crate::types::AiMode::Off;
        snapshot.translation_enabled = settings.processing_translation_enabled;
    }
    Ok(persisted)
}

fn without_secrets(mut settings: Settings) -> Settings {
    settings.llm_api_key = None;
    for profile in &mut settings.llm_profiles {
        profile.api_key = None;
    }
    settings
}

#[cfg(test)]
mod tests;
