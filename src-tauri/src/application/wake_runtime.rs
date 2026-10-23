//! Optional detector lifecycle hooks used by ordinary dictation and diagnostics.
use crate::types::WakeWordStatus;
use tauri::AppHandle;
#[cfg(feature = "wake-word")]
use tauri::Manager;

pub fn pause(app: &AppHandle) {
    super::wake_lira::pause(app);
    #[cfg(feature = "wake-word")]
    app.state::<fono_wake::WakeWordHandle>().pause();
    #[cfg(not(feature = "wake-word"))]
    let _ = app;
}

pub fn resume(app: &AppHandle) {
    super::wake_lira::resume(app);
    #[cfg(feature = "wake-word")]
    app.state::<fono_wake::WakeWordHandle>().resume();
    #[cfg(not(feature = "wake-word"))]
    let _ = app;
}

pub fn status(app: &AppHandle) -> WakeWordStatus {
    #[cfg(feature = "wake-word")]
    {
        app.state::<fono_wake::WakeWordHandle>().status()
    }
    #[cfg(not(feature = "wake-word"))]
    {
        let _ = app;
        WakeWordStatus::Off
    }
}

pub fn is_configuring(app: &AppHandle) -> bool {
    #[cfg(feature = "wake-word")]
    {
        super::wake_calibration::status(app).active || super::wake_validation::status(app).active
    }
    #[cfg(not(feature = "wake-word"))]
    {
        let _ = app;
        false
    }
}
