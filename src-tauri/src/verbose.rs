//! Глобальный флаг подробного логирования.
//!
//! Используется для включения/выключения дополнительных логов без перезапуска.

use std::sync::atomic::{AtomicBool, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(value: bool) {
    VERBOSE.store(value, Ordering::Relaxed);
    tracing::info!("verbose logging {}", if value { "enabled" } else { "disabled" });
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        if $crate::verbose::is_verbose() {
            tracing::info!($($arg)*);
        }
    };
}
