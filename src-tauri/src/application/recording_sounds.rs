//! Optional user cues are serialized against every AudioHub input subscriber.
use crate::{
    audio::cues::{self, Cue},
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
};
use fono_core::OperationSource;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub(crate) struct Runtime {
    error_pending: Arc<AtomicBool>,
}
struct ErrorPermit(Arc<AtomicBool>);
impl Drop for ErrorPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn audible(enabled: bool, volume: u8, source: OperationSource) -> bool {
    enabled
        && (1..=100).contains(&volume)
        && matches!(source, OperationSource::Ui | OperationSource::Hotkey)
}

/// The caller owns a blocking worker and its update activity lease. Start is
/// called before subscribing; Stop only after the subscriber has fully drained.
pub(crate) fn play_for(app: &AppHandle, operation: u64, source: OperationSource, cue: Cue) {
    let settings = app.state::<AppState>().settings();
    if !audible(
        settings.recording_sounds_enabled,
        settings.recording_sounds_volume,
        source,
    ) {
        return;
    }
    let Some(hub) = app.try_state::<fono_audio::AudioHub>() else {
        return;
    };
    hub.with_idle_output(|| {
        let pipeline = app.state::<Pipeline>();
        let current = pipeline.current_operation().map(|op| op.id);
        if !matches_operation(cue, operation, current) {
            return;
        }
        // Re-read after admission: turning sounds off also mutes queued errors.
        let settings = app.state::<AppState>().settings();
        if audible(
            settings.recording_sounds_enabled,
            settings.recording_sounds_volume,
            source,
        ) {
            let _ = cues::play(&[cue], settings.recording_sounds_volume);
        }
    });
}

fn matches_operation(cue: Cue, expected: u64, current: Option<u64>) -> bool {
    current == Some(expected) || (cue == Cue::Error && current.is_none())
}

/// At most one error worker is queued. Its permit and update lease live until
/// actual playback ends even if the publishing IPC future has been dropped.
pub(crate) fn error(app: &AppHandle, operation: u64, source: OperationSource) {
    let settings = app.state::<AppState>().settings();
    if !audible(
        settings.recording_sounds_enabled,
        settings.recording_sounds_volume,
        source,
    ) {
        return;
    }
    let runtime = app.state::<Runtime>();
    if runtime.error_pending.swap(true, Ordering::AcqRel) {
        return;
    }
    let permit = ErrorPermit(runtime.error_pending.clone());
    let Ok(activity) = super::updates::activity::lease() else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_permit, _activity) = (permit, activity);
        play_for(&app, operation, source, Cue::Error);
    });
}

/// Explicit draft-volume preview; never starts input or changes settings.
pub(crate) fn preview(app: &AppHandle, volume: u8) -> AppResult<()> {
    if volume > 100 {
        return Err(AppError::Config("Выберите громкость от 0 до 100%.".into()));
    }
    if app.state::<Pipeline>().current_operation().is_some() {
        return Err(AppError::Busy("Сначала завершите текущую диктовку.".into()));
    }
    let hub = app.state::<fono_audio::AudioHub>();
    match hub.with_idle_output(|| cues::play(&[Cue::Start, Cue::Stop, Cue::Error], volume)) {
        None => Err(AppError::Busy(
            "Сначала завершите запись или проверку микрофона.".into(),
        )),
        Some(false) => Err(AppError::Audio(
            "Не удалось воспроизвести звуковые сигналы.".into(),
        )),
        Some(true) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_settings_are_silent_and_disabling_preserves_the_volume() {
        let mut legacy = serde_json::to_value(crate::types::Settings::default()).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("recording_sounds_enabled");
        legacy
            .as_object_mut()
            .unwrap()
            .remove("recording_sounds_volume");
        let mut settings: crate::types::Settings = serde_json::from_value(legacy).unwrap();
        assert!(!settings.recording_sounds_enabled);
        assert_eq!(settings.recording_sounds_volume, 25);
        for volume in [0, 25, 100] {
            settings.recording_sounds_volume = volume;
            assert!(settings.validate_recording_sounds().is_ok());
        }
        settings.recording_sounds_volume = 101;
        assert!(settings.validate_recording_sounds().is_err());
        settings.recording_sounds_volume = 75;
        settings.recording_sounds_enabled = false;
        let reloaded: crate::types::Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!reloaded.recording_sounds_enabled);
        assert_eq!(reloaded.recording_sounds_volume, 75);
    }
    #[test]
    fn disabled_zero_invalid_and_diagnostic_cues_are_silent() {
        assert!(!audible(false, 25, OperationSource::Hotkey));
        assert!(!audible(true, 0, OperationSource::Ui));
        assert!(!audible(true, 101, OperationSource::Ui));
        assert!(!audible(true, 25, OperationSource::Diagnostics));
        assert!(audible(true, 25, OperationSource::Hotkey));
    }
    #[test]
    fn old_cues_cannot_sound_for_a_replacement_operation() {
        for cue in [Cue::Start, Cue::Stop, Cue::Error] {
            assert!(!matches_operation(cue, 7, Some(8)));
            assert!(matches_operation(cue, 7, Some(7)));
        }
        assert!(!matches_operation(Cue::Start, 7, None));
        assert!(!matches_operation(Cue::Stop, 7, None));
        assert!(matches_operation(Cue::Error, 7, None));
        let pending = Arc::new(AtomicBool::new(true));
        drop(ErrorPermit(pending.clone()));
        assert!(!pending.load(Ordering::Acquire));
    }
}
