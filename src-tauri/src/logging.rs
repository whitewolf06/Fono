//! Process logging and fail-safe crash reporting.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub(crate) fn install_crash_hook() {
    crate::crash_log::install(|| {
        crate::state::app_data_dir()
            .ok()
            .map(|dir| dir.join("logs"))
    });
}

/// Инициализация логирования: консоль + файл в `%APPDATA%\Fono\logs\`.
pub(crate) fn init() {
    use tracing_appender::rolling;

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,fono=debug"));

    // Файловый appender: один файл в день, храним не более двух недель.
    let log_dir = crate::state::app_data_dir()
        .map(|d| d.join("logs"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = rolling::RollingFileAppender::builder()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("fono.log")
        .max_log_files(14)
        .build(&log_dir);
    let Ok(file_appender) = file_appender else {
        // A read-only/full profile must not prevent dictation from starting.
        let _ = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_target(false).with_file(false))
            .try_init();
        tracing::warn!("file logging unavailable; using console logging");
        return;
    };
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    // guard нужно держать вечно — отдадим в leak, чтобы не дропнуть.
    std::mem::forget(guard);

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false) // без цветовых ESC-кодов в файле
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    let console_layer = fmt::layer().with_target(false).with_file(false);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .try_init();

    tracing::info!("log file: {}", log_dir.join("fono.log").display());
}
