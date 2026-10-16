//! Explicit, cancellable period coaching. Local report generation never calls LLM.
mod payload;
mod requests;
pub(crate) use requests::SessionRequest;
pub(crate) use requests::{
    invalidate_all_sessions, invalidate_session, settings_changed, sources_changed,
};

use crate::{
    application::{
        trainer_report::{
            self, TrainerAiConfiguration, TrainerAiContextPreflight, TrainerAiPriority,
            TrainerAiSummary, TrainerReport,
        },
        updates::activity,
    },
    error::{AppError, AppResult},
    state::AppState,
    types::{LlmConnectionKind, LlmProfile, Settings, SpeechLlmDataScope},
};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

fn effective_scope(settings: &Settings, share_text: bool) -> SpeechLlmDataScope {
    if share_text {
        settings.speech_analysis_llm.data_scope
    } else {
        SpeechLlmDataScope::MetricsOnly
    }
}
fn configuration(settings: &Settings) -> TrainerAiConfiguration {
    let assignment = &settings.speech_analysis_llm;
    let profile = settings.speech_analysis_profile();
    TrainerAiConfiguration {
        enabled: settings.history_enabled
            && settings.analytics_enabled
            && settings.speech_trainer_enabled
            && assignment.enabled,
        profile_id: profile.map(|profile| profile.id.clone()),
        profile_name: profile.map(|profile| profile.name.clone()),
        model: assignment
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            .or_else(|| {
                profile
                    .and_then(|profile| profile.model.as_deref())
                    .filter(|model| !model.trim().is_empty())
            })
            .map(|model| model.trim().to_owned()),
        cloud: profile.is_some_and(|profile| profile.connection == LlmConnectionKind::Cloud),
        cloud_consent: assignment.cloud_consent,
        data_scope: assignment.data_scope,
    }
}
pub(crate) fn ai_fingerprint(
    snapshot_fingerprint: &str,
    settings: &Settings,
    scope: SpeechLlmDataScope,
) -> AppResult<String> {
    let value = serde_json::json!({
        "snapshot": snapshot_fingerprint,
        "configuration": configuration(settings),
        "profile": settings.speech_analysis_profile(),
        "speechAssignment": settings.speech_analysis_llm,
        "requestedScope": scope,
        "parameters": {
            "promptVersion": payload::PROMPT_VERSION, "temperature": 0.2, "maxTokens": 1400,
            "maxSharedSessions": payload::MAX_SHARED_SESSIONS,
            "maxCharactersPerSession": payload::MAX_SHARED_CHARS,
            "maxPatterns": payload::MAX_PATTERNS,
            "maxExamplesPerPattern": payload::MAX_EXAMPLES_PER_PATTERN,
            "maxPayloadBytes": payload::MAX_PAYLOAD_BYTES,
            "wordBoundary": "unicode-lexical-prefix-1"
        }
    });
    let bytes = serde_json::to_vec(&value)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(crate) fn report(days: u32, refresh: bool, settings: &Settings) -> AppResult<TrainerReport> {
    let (state, sources) = trainer_report::get_with_sources(days, refresh, settings)?;
    let configuration = configuration(settings);
    let scope = state
        .ai_summary
        .as_ref()
        .map(|summary| summary.data_scope)
        .unwrap_or(SpeechLlmDataScope::MetricsOnly);
    let ai_fingerprint = ai_fingerprint(&state.current_fingerprint, settings, scope)?;
    let ai_stale = state
        .ai_summary
        .as_ref()
        .is_some_and(|summary| summary.fingerprint != ai_fingerprint);
    let reason = if state.stale {
        Some("Обновите локальный отчёт: данные периода изменились")
    } else if !configuration.enabled {
        Some("Включите историю, аналитику, речевого тренера и рекомендации ИИ")
    } else if configuration.profile_id.is_none() {
        Some("Выберите подключение речевого тренера")
    } else if configuration.model.is_none() {
        Some("Выберите модель речевого тренера")
    } else if configuration.cloud && !configuration.cloud_consent {
        Some("Подтвердите передачу выбранных данных в облако в настройках")
    } else if state.snapshot.included_sessions == 0 {
        Some("В выбранном периоде нет готовых включённых диктовок")
    } else if state.snapshot.insufficient_data {
        Some("Для ИИ-отчёта нужны минимум 3 готовые диктовки и 100 слов")
    } else {
        None
    };
    let ai_context = if state.stale {
        None
    } else {
        Some(TrainerAiContextPreflight {
            metrics_only: payload::prepare(
                &state.snapshot,
                &sources,
                SpeechLlmDataScope::MetricsOnly,
            )?
            .context,
            configured_shared: if configuration.data_scope == SpeechLlmDataScope::MetricsOnly {
                None
            } else {
                Some(payload::prepare(&state.snapshot, &sources, configuration.data_scope)?.context)
            },
        })
    };
    Ok(TrainerReport {
        snapshot: state.snapshot,
        current_fingerprint: state.current_fingerprint,
        stale: state.stale,
        ai_fingerprint,
        ai_stale,
        ai_summary: state.ai_summary,
        ai_configuration: configuration,
        ai_disabled_reason: reason.map(str::to_owned),
        ai_context,
    })
}

pub(crate) fn cancel(request_id: &str) {
    requests::cancel_period(request_id);
}

/// The existing one-session action uses the same privacy and actual-work fence.
pub(crate) async fn recommend_session(
    app: AppHandle,
    id: String,
) -> AppResult<crate::llm::SpeechLlmRecommendation> {
    let state = app.state::<AppState>();
    let (settings, revision) = {
        let settings = state.settings.lock();
        (settings.clone(), state.settings_revision())
    };
    let cfg = configuration(&settings);
    if !cfg.enabled {
        return Err(AppError::Config(
            "Включите тренера, аналитику и рекомендации ИИ".into(),
        ));
    }
    if cfg.cloud && !cfg.cloud_consent {
        return Err(AppError::Config(
            "Подтвердите передачу данных в облако".into(),
        ));
    }
    let profile = settings
        .speech_analysis_profile()
        .ok_or_else(|| AppError::Config("Выберите подключение тренера".into()))?;
    let lease = activity::lease()?;
    let request = SessionRequest::begin(&id)?;
    let (original, analysis, source_fingerprint) = session_input(&id, &settings)?;
    let client = client_for_profile(
        profile,
        settings.speech_analysis_llm.model.as_deref(),
        &lease,
    )
    .await?;
    let task = tauri::async_runtime::spawn(async move {
        let _lease = lease;
        if app.state::<AppState>().settings_revision() != revision {
            return Err(AppError::Cancelled(
                "Настройки изменились до начала анализа".into(),
            ));
        }
        let scope = settings.speech_analysis_llm.data_scope;
        let findings =
            (scope != SpeechLlmDataScope::MetricsOnly).then_some(analysis.findings.as_slice());
        let text = (scope == SpeechLlmDataScope::OriginalText).then_some(original.as_str());
        let recommendation = tokio::select! {
            biased;
            _ = request.token.cancelled() => return Err(AppError::Cancelled("Анализ диктовки отменён".into())),
            result = client.analyze_speech(&analysis, findings, text) => result?,
        };
        let state = app.state::<AppState>();
        let current = state.settings.lock();
        if request.token.is_cancelled()
            || state.settings_revision() != revision
            || session_input(&id, &current)?.2 != source_fingerprint
        {
            return Err(AppError::Cancelled(
                "Диктовка или настройки изменились. Результат отклонён".into(),
            ));
        }
        Ok(recommendation)
    });
    task.await
        .map_err(|_| AppError::Internal("Не удалось завершить анализ диктовки".into()))?
}

fn session_input(
    id: &str,
    settings: &Settings,
) -> AppResult<(String, crate::types::SpeechSessionAnalysis, String)> {
    crate::history::with_trainer_entries(settings, |entries| {
        let entry = entries
            .iter()
            .find(|entry| {
                entry.id == id && trainer_report::eligible(entry, settings, chrono::Utc::now())
            })
            .ok_or_else(|| AppError::Cancelled("Диктовка недоступна для анализа".into()))?;
        Ok((
            entry.original_text.clone().unwrap_or_default(),
            entry
                .analysis
                .clone()
                .ok_or_else(|| AppError::Config("Дождитесь локального анализа диктовки".into()))?,
            trainer_report::source_fingerprint(entry)?,
        ))
    })
}

pub(crate) async fn analyze(
    app: AppHandle,
    days: u32,
    fingerprint: String,
    share_text: bool,
    request_id: String,
) -> AppResult<TrainerReport> {
    let lease = activity::lease()?;
    let state = app.state::<AppState>();
    let (settings, settings_revision, epoch) = {
        let settings = state.settings.lock();
        (
            settings.clone(),
            state.settings_revision(),
            requests::source_epoch(),
        )
    };
    let current = report(days, false, &settings)?;
    if current.stale || current.current_fingerprint != fingerprint {
        return Err(AppError::Cancelled(
            "Данные периода изменились. Обновите локальный отчёт".into(),
        ));
    }
    if let Some(reason) = current.ai_disabled_reason {
        return Err(AppError::Config(reason));
    }
    let scope = effective_scope(&settings, share_text);
    let identity = ai_fingerprint(&fingerprint, &settings, scope)?;
    // Scope is part of identity: changing the explicit text choice can create a
    // new report, while an identical snapshot/configuration reuses the saved one.
    if current
        .ai_summary
        .as_ref()
        .is_some_and(|summary| summary.fingerprint == identity)
    {
        return report(days, false, &settings);
    }

    // The detached job owns admission until its HTTP future and local commit
    // actually finish, including the local context preparation above.
    let request = requests::PeriodRequest::begin(request_id, days)?;
    let (snapshot, sources) = trainer_report::analysis_input(days, &fingerprint, &settings)?;
    let prepared = payload::prepare(&snapshot, &sources, scope)?;
    let input = prepared.input;
    let context = prepared.context;
    let profile = settings
        .speech_analysis_profile()
        .ok_or_else(|| AppError::Config("Выберите подключение речевого тренера".into()))?;
    let client = client_for_profile(
        profile,
        settings.speech_analysis_llm.model.as_deref(),
        &lease,
    )
    .await?;
    let profile_id = profile.id.clone();
    let profile_name = profile.name.clone();
    let model = client
        .selected_model()
        .ok_or_else(|| AppError::Config("Выберите модель речевого тренера".into()))?
        .to_owned();
    let task = tauri::async_runtime::spawn(async move {
        let _lease = lease;
        if app.state::<AppState>().settings_revision() != settings_revision {
            return Err(AppError::Cancelled(
                "Настройки изменились до начала анализа".into(),
            ));
        }
        let result = tokio::select! {
            biased;
            _ = request.token.cancelled() => return Err(AppError::Cancelled("Анализ отчёта отменён".into())),
            _ = requests::changed_since(epoch) => return Err(AppError::Cancelled("Данные или настройки изменились. Повторите анализ после обновления".into())),
            result = client.analyze_speech_period(&input) => result?,
        };
        request.commit(|| {
            let state = app.state::<AppState>();
            let settings = state.settings.lock();
            if state.settings_revision() != settings_revision
                || requests::source_epoch() != epoch
                || ai_fingerprint(&fingerprint, &settings, scope)? != identity
            {
                return Err(AppError::Cancelled(
                    "Данные или настройки изменились. Результат не сохранён".into(),
                ));
            }
            let summary = TrainerAiSummary {
                fingerprint: identity,
                snapshot_fingerprint: fingerprint.clone(),
                generated_at: chrono::Utc::now().to_rfc3339(),
                profile_id,
                profile_name,
                model,
                data_scope: scope,
                context: Some(context),
                summary: result.summary,
                priorities: result
                    .priorities
                    .into_iter()
                    .map(|item| TrainerAiPriority {
                        title: item.title,
                        basis: item.basis,
                        exercise: item.exercise,
                    })
                    .collect(),
            };
            trainer_report::save_ai(days, &fingerprint, summary, &settings)?;
            report(days, false, &settings)
        })
    });
    task.await
        .map_err(|_| AppError::Internal("Не удалось завершить анализ отчёта".into()))?
}

async fn client_for_profile(
    profile: &LlmProfile,
    model: Option<&str>,
    lease: &activity::ActivityLease<'static>,
) -> AppResult<crate::llm::LlmClient> {
    let profile = profile.clone();
    let model = model.map(str::to_owned);
    let lease = lease.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Keep update admission while the native keyring read actually runs,
        // including when the surrounding IPC future has already been dropped.
        let _lease = lease;
        crate::llm::LlmClient::from_profile(&profile, model.as_deref())
    })
    .await
    .map_err(|_| AppError::Internal("LLM client preparation task failed".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LlmProfile, LlmProvider, SpeechLlmAssignment};

    fn settings() -> Settings {
        Settings {
            history_enabled: true,
            analytics_enabled: true,
            speech_trainer_enabled: true,
            llm_profiles: vec![LlmProfile {
                id: "trainer".into(),
                name: "Речевой тренер".into(),
                provider: LlmProvider::Custom,
                connection: LlmConnectionKind::Local,
                base_url: "http://127.0.0.1:1234/v1".into(),
                model: Some("model-a".into()),
                api_key: None,
                has_api_key: false,
            }],
            speech_analysis_llm: SpeechLlmAssignment {
                enabled: true,
                profile_id: Some("trainer".into()),
                model: None,
                data_scope: SpeechLlmDataScope::OriginalText,
                cloud_consent: false,
            },
            ..Settings::default()
        }
    }
    #[test]
    fn explicit_text_choice_cannot_escalate_the_saved_scope() {
        let mut settings = settings();
        assert_eq!(
            effective_scope(&settings, false),
            SpeechLlmDataScope::MetricsOnly
        );
        settings.speech_analysis_llm.data_scope = SpeechLlmDataScope::Findings;
        assert_eq!(
            effective_scope(&settings, true),
            SpeechLlmDataScope::Findings
        );
        settings.speech_analysis_llm.data_scope = SpeechLlmDataScope::MetricsOnly;
        assert_eq!(
            effective_scope(&settings, true),
            SpeechLlmDataScope::MetricsOnly
        );
    }
    #[test]
    fn ai_identity_changes_with_sources_model_profile_scope_and_parameters() {
        let mut settings = settings();
        let original =
            ai_fingerprint("source-a", &settings, SpeechLlmDataScope::MetricsOnly).unwrap();
        assert_eq!(
            original,
            ai_fingerprint("source-a", &settings, SpeechLlmDataScope::MetricsOnly).unwrap()
        );
        assert_ne!(
            original,
            ai_fingerprint("source-b", &settings, SpeechLlmDataScope::MetricsOnly).unwrap()
        );
        assert_ne!(
            original,
            ai_fingerprint("source-a", &settings, SpeechLlmDataScope::OriginalText).unwrap()
        );
        settings.speech_analysis_llm.model = Some("model-b".into());
        assert_ne!(
            original,
            ai_fingerprint("source-a", &settings, SpeechLlmDataScope::MetricsOnly).unwrap()
        );
        settings.speech_analysis_llm.model = None;
        settings.llm_profiles[0].base_url = "http://127.0.0.1:9876/v1".into();
        assert_ne!(
            original,
            ai_fingerprint("source-a", &settings, SpeechLlmDataScope::MetricsOnly).unwrap()
        );
    }
    #[test]
    fn fingerprint_never_serializes_credential_contents() {
        let mut settings = settings();
        let original =
            ai_fingerprint("source", &settings, SpeechLlmDataScope::MetricsOnly).unwrap();
        settings.llm_profiles[0].api_key = Some("private-test-key-never-persist".into());
        assert_eq!(
            original,
            ai_fingerprint("source", &settings, SpeechLlmDataScope::MetricsOnly).unwrap()
        );
    }
}
