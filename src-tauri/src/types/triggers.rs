//! Persisted trigger selection. RegisterHotKey remains the compatibility default.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationTrigger {
    #[default]
    Shortcut,
    RightAlt,
    RightCtrl,
    CapsLock,
    XButton1,
    XButton2,
}

impl DictationTrigger {
    #[cfg(any(windows, test))]
    pub(crate) fn is_keyboard(self) -> bool {
        matches!(self, Self::RightAlt | Self::RightCtrl | Self::CapsLock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Settings;

    #[test]
    fn old_settings_keep_register_hotkey_and_every_trigger_roundtrips() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.dictation_trigger, DictationTrigger::Shortcut);
        for trigger in [
            DictationTrigger::Shortcut,
            DictationTrigger::RightAlt,
            DictationTrigger::RightCtrl,
            DictationTrigger::CapsLock,
            DictationTrigger::XButton1,
            DictationTrigger::XButton2,
        ] {
            let settings = Settings {
                dictation_trigger: trigger,
                ..old.clone()
            };
            let json = serde_json::to_value(settings).unwrap();
            let restored: Settings = serde_json::from_value(json).unwrap();
            assert_eq!(restored.dictation_trigger, trigger);
        }
        assert!(serde_json::from_str::<DictationTrigger>("\"arbitrary_key\"").is_err());
    }
}
