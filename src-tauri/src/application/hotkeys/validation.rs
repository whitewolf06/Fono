use crate::{
    error::{AppError, AppResult},
    types::{DictationTrigger, Settings},
};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

pub(crate) fn validate(settings: &Settings) -> AppResult<()> {
    let parse = |value: &str| -> AppResult<Shortcut> {
        let key = value
            .parse::<Shortcut>()
            .map_err(|_| AppError::Config("Укажите корректное сочетание клавиш.".into()))?;
        if key.mods == Modifiers::ALT && key.key == Code::F4 {
            return Err(AppError::Config(
                "Alt + F4 закрывает окно. Выберите другое сочетание.".into(),
            ));
        }
        Ok(key)
    };
    let command = parse(&settings.command_hotkey)?;
    if settings.selection_capture_enabled {
        let selection = parse(&settings.selection_hotkey)?;
        let overlap = selection.id() == command.id()
            || (settings.dictation_trigger == DictationTrigger::Shortcut
                && parse(&settings.hotkey)?.id() == selection.id())
            || (settings.dictation_trigger == DictationTrigger::RightCtrl
                && selection.mods.contains(Modifiers::CONTROL))
            || (settings.dictation_trigger == DictationTrigger::RightAlt
                && selection.mods.contains(Modifiers::ALT))
            || (settings.dictation_trigger == DictationTrigger::CapsLock
                && selection.key == Code::CapsLock);
        if overlap {
            return Err(AppError::Config("Сочетание выделения конфликтует с диктовкой или голосовыми командами. Выберите другое сочетание.".into()));
        }
    }
    match settings.dictation_trigger {
        DictationTrigger::Shortcut if parse(&settings.hotkey)?.id() == command.id() => {
            return Err(AppError::Config(
                "Клавиши диктовки и команд должны отличаться.".into(),
            ))
        }
        DictationTrigger::RightCtrl if command.mods.contains(Modifiers::CONTROL) => {
            return Err(overlap())
        }
        DictationTrigger::RightAlt if command.mods.contains(Modifiers::ALT) => {
            return Err(overlap())
        }
        DictationTrigger::CapsLock if command.key == Code::CapsLock => return Err(overlap()),
        _ => {}
    }
    Ok(())
}

fn overlap() -> AppError {
    AppError::Config("Кнопка диктовки входит в сочетание команд. Измените сочетание команд или выберите другую кнопку.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn selection_shortcut_checks_both_register_hotkeys_and_single_modifiers() {
        let mut s = Settings {
            selection_capture_enabled: true,
            ..Settings::default()
        };
        assert!(validate(&s).is_ok());
        s.selection_hotkey = "Shift+Control+Space".into();
        assert!(validate(&s).is_err());
        s.selection_hotkey = "Ctrl+Space".into();
        assert!(validate(&s).is_err());
        s.selection_hotkey = "Ctrl+Alt+Space".into();
        s.dictation_trigger = DictationTrigger::RightAlt;
        assert!(validate(&s).is_err());
        s.selection_hotkey = "Ctrl+Shift+E".into();
        assert!(validate(&s).is_ok());
        s.selection_capture_enabled = false;
        s.selection_hotkey = "unused legacy".into();
        assert!(validate(&s).is_ok());
    }
    use super::*;
    #[test]
    fn compares_parsed_shortcuts_and_rejects_reserved_close() {
        let mut s = Settings {
            hotkey: "Shift+Ctrl+Space".into(),
            command_hotkey: "Control+Shift+Space".into(),
            ..Settings::default()
        };
        assert!(validate(&s).is_err());
        s.command_hotkey = "Alt+F4".into();
        assert!(validate(&s).is_err());
        s.command_hotkey = "invalid".into();
        assert!(validate(&s).is_err());
    }
    #[test]
    fn checks_only_active_trigger_and_rejects_modifier_overlap() {
        let mut s = Settings {
            dictation_trigger: DictationTrigger::XButton1,
            hotkey: "unused legacy value".into(),
            ..Settings::default()
        };
        assert!(validate(&s).is_ok());
        s.dictation_trigger = DictationTrigger::RightCtrl;
        assert!(validate(&s).is_err());
        s.command_hotkey = "Alt+F9".into();
        assert!(validate(&s).is_ok());
        s.dictation_trigger = DictationTrigger::RightAlt;
        assert!(validate(&s).is_err());
        s.command_hotkey = "Ctrl+Shift+Space".into();
        assert!(validate(&s).is_ok());
    }
}
