//! Consent is captured before reading a field; later settings can only narrow it.
use super::*;
use crate::types::{DictationAnalysisStatus, DictationHistoryEntry, Settings};

#[derive(Default)]
pub(super) struct Policy {
    pub auto_insert: bool,
    pub history_enabled: bool,
    pub message: Option<String>,
    archived: bool,
}
impl Policy {
    pub(super) fn from_settings(settings: &Settings) -> Self {
        Self {
            auto_insert: settings.selection_auto_insert,
            history_enabled: settings.selection_history_enabled && settings.history_enabled,
            ..Self::default()
        }
    }
    pub(super) fn auto_allowed(&self, current: &Settings) -> bool {
        self.auto_insert && current.selection_auto_insert && current.selection_capture_enabled
    }
    fn claim(&mut self, current: &Settings) -> bool {
        if self.archived
            || !self.history_enabled
            || !current.history_enabled
            || !current.selection_history_enabled
        {
            return false;
        }
        self.archived = true;
        true
    }
}
fn entry(text: String) -> DictationHistoryEntry {
    DictationHistoryEntry {
        id: crate::history::next_id(),
        text,
        created_at: chrono::Utc::now(),
        device: None,
        metadata: None,
        analytics_included: false,
        original_text: None,
        processing: None,
        analysis_status: DictationAnalysisStatus::Disabled,
        analysis: None,
        analysis_error: None,
        recommendation_status: DictationAnalysisStatus::Disabled,
        recommendation: None,
        recommendation_error: None,
    }
}
/// Called on the existing blocking delivery worker, under the selection fence.
/// Holding current settings until append linearizes revocation with this write.
pub(super) fn archive(app: &AppHandle, active: &mut Active, result: u64) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock();
    if !active.privacy.claim(&settings) {
        return;
    }
    let Ok(text) = active.edit.output(result, false) else {
        return;
    };
    if crate::history::for_app(app)
        .append(entry(text), false, settings.analytics_retention_days)
        .is_err()
    {
        // No raw disk error, path, instruction or selected text enters logs/events.
        active.privacy.message = Some("Результат готов, но сохранить его в историю не удалось. Скопируйте текст перед закрытием.".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_legacy_settings_never_opt_in() {
        let settings: Settings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(!settings.selection_auto_insert);
        assert!(!settings.selection_history_enabled);
        let mut policy = Policy::from_settings(&settings);
        let enabled = Settings {
            selection_auto_insert: true,
            selection_capture_enabled: true,
            selection_history_enabled: true,
            history_enabled: true,
            ..settings
        };
        assert!(!policy.auto_allowed(&enabled));
        assert!(!policy.claim(&enabled));
    }
    #[test]
    fn later_revocation_wins_and_one_accepted_result_archives_once() {
        let mut settings = Settings {
            selection_auto_insert: true,
            selection_capture_enabled: true,
            selection_history_enabled: true,
            history_enabled: true,
            ..Default::default()
        };
        let mut policy = Policy::from_settings(&settings);
        settings.selection_auto_insert = false;
        settings.selection_history_enabled = false;
        assert!(!policy.auto_allowed(&settings));
        assert!(!policy.claim(&settings));
        settings.selection_history_enabled = true;
        settings.history_enabled = false;
        assert!(!policy.claim(&settings));
        settings.history_enabled = true;
        assert!(policy.claim(&settings));
        assert!(!policy.claim(&settings));
    }
    #[test]
    fn archive_has_final_text_but_no_material_instruction_or_invented_stt_facts() {
        let value = entry("accepted result".into());
        assert_eq!(value.text, "accepted result");
        assert!(value.original_text.is_none());
        assert!(value.metadata.is_none());
        assert!(value.processing.is_none());
        assert!(value.device.is_none());
        assert!(!value.analytics_included);
        assert_eq!(value.analysis_status, DictationAnalysisStatus::Disabled);
        assert_eq!(
            value.recommendation_status,
            DictationAnalysisStatus::Disabled
        );
    }
}
