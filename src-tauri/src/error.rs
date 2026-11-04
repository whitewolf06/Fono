//! Типы ошибок приложения.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserErrorCode {
    Busy,
    Cancelled,
    Audio,
    Stt,
    Llm,
    Injection,
    Io,
    Json,
    Config,
    ModelNotLoaded,
    ModelNotSelected,
    ModelMissing,
    BackendUnavailable,
    BackendStartFailed,
    Download,
    Settings,
    Wake,
    Internal,
}

impl UserErrorCode {
    fn key(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Cancelled => "cancelled",
            Self::Audio => "audio",
            Self::Stt => "stt",
            Self::Llm => "llm",
            Self::Injection => "injection",
            Self::Io => "io",
            Self::Json => "json",
            Self::Config => "config",
            Self::ModelNotLoaded => "model_not_loaded",
            Self::ModelNotSelected => "model_not_selected",
            Self::ModelMissing => "model_missing",
            Self::BackendUnavailable => "backend_unavailable",
            Self::BackendStartFailed => "backend_start_failed",
            Self::Download => "download",
            Self::Settings => "settings",
            Self::Wake => "wake",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionBackend {
    Auto,
    Cpu,
    Cuda,
    Vulkan,
}

impl RecognitionBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Авто",
            Self::Cpu => "Процессор",
            Self::Cuda => "CUDA",
            Self::Vulkan => "Vulkan",
        }
    }
}

#[derive(Debug, Default, serde::Serialize)]
pub struct UserErrorParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<RecognitionBackend>,
}

/// The IPC error contract contains no raw source error, path or arbitrary data.
#[derive(Debug, serde::Serialize)]
pub struct UserError {
    pub code: UserErrorCode,
    pub params: UserErrorParams,
}

#[derive(serde::Deserialize)]
struct UserErrorCatalog {
    messages: std::collections::BTreeMap<String, String>,
    templates: std::collections::BTreeMap<String, String>,
    details: Vec<String>,
}

fn catalog() -> Option<&'static UserErrorCatalog> {
    static CATALOG: std::sync::OnceLock<Option<UserErrorCatalog>> = std::sync::OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../src/v3/shared/infrastructure/native/userErrorCatalog.json"
            ))
            .ok()
        })
        .as_ref()
}

fn trusted_detail(raw: &str) -> Option<String> {
    // Exact authored messages are allowlisted. Cyrillic text, a translated
    // prefix, or a message with a secret/path appended does not grant trust.
    let mut raw = raw.trim();
    const LEGACY_PREFIXES: &[&str] = &[
        "операция уже выполняется: ",
        "операция отменена: ",
        "аудио ошибка: ",
        "ошибка whisper/stt: ",
        "ошибка LLM: ",
        "ошибка вставки текста: ",
        "ошибка ввода-вывода: ",
        "ошибка JSON: ",
        "ошибка конфигурации: ",
        "внутренняя ошибка: ",
    ];
    for _ in 0..3 {
        let Some(detail) = LEGACY_PREFIXES
            .iter()
            .find_map(|prefix| raw.strip_prefix(prefix))
        else {
            break;
        };
        raw = detail.trim();
    }
    if raw.len() > 1200 {
        return None;
    }
    let catalog = catalog()?;
    let allowed = catalog.details.iter().any(|allowed| allowed == raw)
        || catalog.messages.values().any(|allowed| allowed == raw)
        || catalog.templates.values().any(|template| {
            [
                RecognitionBackend::Auto,
                RecognitionBackend::Cpu,
                RecognitionBackend::Cuda,
                RecognitionBackend::Vulkan,
            ]
            .iter()
            .any(|backend| template.replace("{backend}", backend.label()) == raw)
        });
    allowed.then(|| raw.to_owned())
}

/// Legacy events still carry text; use the same safe catalogue as IPC mapping.
pub fn safe_user_message(code: UserErrorCode, raw: &str) -> String {
    trusted_detail(raw).unwrap_or_else(|| default_user_message(code, None))
}

fn default_user_message(code: UserErrorCode, backend: Option<RecognitionBackend>) -> String {
    if let (Some(catalog), Some(backend)) = (catalog(), backend) {
        if let Some(template) = catalog.templates.get(code.key()) {
            return template.replace("{backend}", backend.label());
        }
    }
    catalog()
        .and_then(|catalog| catalog.messages.get(code.key()))
        .cloned()
        .unwrap_or_else(|| "Не удалось выполнить действие. Перезапустите Fono.".into())
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("операция уже выполняется: {0}")]
    Busy(String),

    #[error("операция отменена: {0}")]
    Cancelled(String),

    #[error("аудио ошибка: {0}")]
    Audio(String),

    #[error("ошибка whisper/stt: {0}")]
    Stt(String),

    #[error("ошибка LLM: {0}")]
    Llm(String),

    #[error("ошибка вставки текста: {0}")]
    Injection(String),

    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("ошибка JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("ошибка конфигурации: {0}")]
    Config(String),

    #[error("модель не загружена")]
    ModelNotLoaded,

    #[error(
        "Выберите модель распознавания в настройках. Если модели ещё нет, сначала загрузите её."
    )]
    ModelNotSelected,

    #[error(
        "Файл выбранной модели не найден. Загрузите модель или выберите другой файл в настройках."
    )]
    ModelFileMissing,

    #[error("STT backend is not packaged: {0:?}")]
    BackendUnavailable(RecognitionBackend),

    #[error("STT backend could not start: {0:?}")]
    BackendStartFailed(RecognitionBackend),

    #[error("внутренняя ошибка: {0}")]
    Internal(String),
}

impl AppError {
    pub fn user_error(&self) -> UserError {
        let (code, raw, backend) = match self {
            Self::Busy(message) => (UserErrorCode::Busy, Some(message), None),
            Self::Cancelled(message) => (UserErrorCode::Cancelled, Some(message), None),
            Self::Audio(message) => (UserErrorCode::Audio, Some(message), None),
            Self::Stt(message) => (UserErrorCode::Stt, Some(message), None),
            Self::Llm(message) => (UserErrorCode::Llm, Some(message), None),
            Self::Injection(message) => (UserErrorCode::Injection, Some(message), None),
            Self::Config(message) => (UserErrorCode::Config, Some(message), None),
            Self::Io(_) => (UserErrorCode::Io, None, None),
            Self::Json(_) => (UserErrorCode::Json, None, None),
            Self::ModelNotLoaded => (UserErrorCode::ModelNotLoaded, None, None),
            Self::ModelNotSelected => (UserErrorCode::ModelNotSelected, None, None),
            Self::ModelFileMissing => (UserErrorCode::ModelMissing, None, None),
            Self::BackendUnavailable(backend) => {
                (UserErrorCode::BackendUnavailable, None, Some(*backend))
            }
            Self::BackendStartFailed(backend) => {
                (UserErrorCode::BackendStartFailed, None, Some(*backend))
            }
            Self::Internal(_) => (UserErrorCode::Internal, None, None),
        };
        UserError {
            code,
            params: UserErrorParams {
                detail: raw.and_then(|raw| trusted_detail(raw)),
                backend,
            },
        }
    }

    pub fn user_message(&self) -> String {
        let error = self.user_error();
        error
            .params
            .detail
            .unwrap_or_else(|| default_user_message(error.code, error.params.backend))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<fono_audio::AudioError> for AppError {
    fn from(error: fono_audio::AudioError) -> Self {
        AppError::Audio(error.to_string())
    }
}

impl From<fono_stt::error::RecognitionBackend> for RecognitionBackend {
    fn from(backend: fono_stt::error::RecognitionBackend) -> Self {
        match backend {
            fono_stt::error::RecognitionBackend::Auto => Self::Auto,
            fono_stt::error::RecognitionBackend::Cpu => Self::Cpu,
            fono_stt::error::RecognitionBackend::Cuda => Self::Cuda,
            fono_stt::error::RecognitionBackend::Vulkan => Self::Vulkan,
        }
    }
}

impl From<fono_stt::error::SttError> for AppError {
    fn from(error: fono_stt::error::SttError) -> Self {
        use fono_stt::error::SttError as E;
        match error {
            E::Remote(error) => Self::Stt(error.to_string()),
            E::Busy(message) => Self::Busy(message),
            E::Cancelled(message) => Self::Cancelled(message),
            E::Audio(message) => Self::Audio(message),
            E::Stt(message) => Self::Stt(message),
            E::Io(error) => Self::Io(error),
            E::Json(error) => Self::Json(error),
            E::Config(message) => Self::Config(message),
            E::ModelNotLoaded => Self::ModelNotLoaded,
            E::ModelNotSelected => Self::ModelNotSelected,
            E::ModelFileMissing => Self::ModelFileMissing,
            E::BackendUnavailable(backend) => Self::BackendUnavailable(backend.into()),
            E::BackendStartFailed(backend) => Self::BackendStartFailed(backend.into()),
            E::Internal(message) => Self::Internal(message),
        }
    }
}

pub(crate) fn stt_user_message(error: &fono_stt::error::SttError) -> String {
    use fono_stt::error::SttError as E;
    let (code, raw, backend) = match error {
        E::Remote(_) => (UserErrorCode::Stt, None, None),
        E::Busy(message) => (UserErrorCode::Busy, Some(message), None),
        E::Cancelled(message) => (UserErrorCode::Cancelled, Some(message), None),
        E::Audio(message) => (UserErrorCode::Audio, Some(message), None),
        E::Stt(message) => (UserErrorCode::Stt, Some(message), None),
        E::Config(message) => (UserErrorCode::Config, Some(message), None),
        E::Io(_) => (UserErrorCode::Io, None, None),
        E::Json(_) => (UserErrorCode::Json, None, None),
        E::ModelNotLoaded => (UserErrorCode::ModelNotLoaded, None, None),
        E::ModelNotSelected => (UserErrorCode::ModelNotSelected, None, None),
        E::ModelFileMissing => (UserErrorCode::ModelMissing, None, None),
        E::BackendUnavailable(backend) => (
            UserErrorCode::BackendUnavailable,
            None,
            Some((*backend).into()),
        ),
        E::BackendStartFailed(backend) => (
            UserErrorCode::BackendStartFailed,
            None,
            Some((*backend).into()),
        ),
        E::Internal(_) => (UserErrorCode::Internal, None, None),
    };
    raw.and_then(|message| trusted_detail(message))
        .unwrap_or_else(|| default_user_message(code, backend))
}

impl From<AppError> for fono_stt::error::SttError {
    fn from(error: AppError) -> Self {
        use fono_stt::error::RecognitionBackend as B;
        let backend = |backend| match backend {
            RecognitionBackend::Auto => B::Auto,
            RecognitionBackend::Cpu => B::Cpu,
            RecognitionBackend::Cuda => B::Cuda,
            RecognitionBackend::Vulkan => B::Vulkan,
        };
        match error {
            AppError::Busy(message) => Self::Busy(message),
            AppError::Cancelled(message) => Self::Cancelled(message),
            AppError::Audio(message) => Self::Audio(message),
            AppError::Stt(message) => Self::Stt(message),
            AppError::Io(error) => Self::Io(error),
            AppError::Json(error) => Self::Json(error),
            AppError::Config(message) => Self::Config(message),
            AppError::ModelNotLoaded => Self::ModelNotLoaded,
            AppError::ModelNotSelected => Self::ModelNotSelected,
            AppError::ModelFileMissing => Self::ModelFileMissing,
            AppError::BackendUnavailable(value) => Self::BackendUnavailable(backend(value)),
            AppError::BackendStartFailed(value) => Self::BackendStartFailed(backend(value)),
            other => Self::Internal(other.to_string()),
        }
    }
}

impl From<fono_types::CoreError> for AppError {
    fn from(error: fono_types::CoreError) -> Self {
        use fono_types::ErrorCode;

        // Keep the desktop IPC vocabulary and its exact-message allowlist.
        // Core errors may also originate in a remote transport; their message
        // is never permission to publish arbitrary details to the frontend.
        match error.code {
            ErrorCode::ModelNotFound => Self::ModelFileMissing,
            ErrorCode::ModelNotReady => Self::ModelNotLoaded,
            ErrorCode::Cancelled => Self::Cancelled(error.message),
            ErrorCode::Busy => Self::Busy(error.message),
            ErrorCode::InvalidRequest => Self::Config(error.message),
            ErrorCode::UnsupportedAudio => Self::Audio(error.message),
            ErrorCode::WorkerFailed => Self::Stt(error.message),
            ErrorCode::Network | ErrorCode::Unauthorized | ErrorCode::Internal => {
                Self::Internal(error.message)
            }
        }
    }
}

#[cfg(feature = "wake-word")]
impl From<fono_wake::WakeWordError> for AppError {
    fn from(e: fono_wake::WakeWordError) -> Self {
        AppError::Audio(e.to_string())
    }
}

impl From<fono_core::CoordinatorError> for AppError {
    fn from(error: fono_core::CoordinatorError) -> Self {
        match error {
            fono_core::CoordinatorError::Busy(message) => AppError::Busy(message),
            fono_core::CoordinatorError::StaleOperation(operation_id) => {
                AppError::Cancelled(format!("operation {operation_id} is no longer active"))
            }
            fono_core::CoordinatorError::ResourceAlreadyLeased {
                operation_id,
                resource,
            } => AppError::Busy(format!(
                "resource {resource:?} is already leased by operation {operation_id}"
            )),
        }
    }
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.user_error(), s)
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_engine_errors_keep_the_desktop_backend_and_safe_detail_contract() {
        use fono_stt::error::{RecognitionBackend as Backend, SttError};
        let error = AppError::from(SttError::BackendUnavailable(Backend::Cuda));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "backend_unavailable", "params": {"backend": "cuda"}
            })
        );
        let message = "Сначала завершите текущую диктовку";
        let error = AppError::from(SttError::Busy(message.into()));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "busy", "params": {"detail": message}
            })
        );
        let private = SttError::Stt("PRIVATE_KEY C:\\profile\\model.bin".into());
        assert!(!stt_user_message(&private).contains("PRIVATE"));
        assert_eq!(
            serde_json::to_value(AppError::from(private)).unwrap(),
            serde_json::json!({"code":"stt", "params":{}})
        );
    }

    #[test]
    fn shared_json_readiness_uses_the_existing_json_message() {
        let error = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let shared = fono_stt::error::SttError::Json(error);
        assert_eq!(
            stt_user_message(&shared),
            default_user_message(UserErrorCode::Json, None)
        );
    }

    #[test]
    fn core_errors_reuse_desktop_codes_without_exposing_source_messages() {
        use fono_types::{CoreError, ErrorCode};

        for (code, desktop_code) in [
            (ErrorCode::ModelNotFound, "model_missing"),
            (ErrorCode::ModelNotReady, "model_not_loaded"),
            (ErrorCode::Cancelled, "cancelled"),
            (ErrorCode::Busy, "busy"),
            (ErrorCode::InvalidRequest, "config"),
            (ErrorCode::UnsupportedAudio, "audio"),
            (ErrorCode::WorkerFailed, "stt"),
            (ErrorCode::Network, "internal"),
            (ErrorCode::Unauthorized, "internal"),
            (ErrorCode::Internal, "internal"),
        ] {
            for retryable in [false, true] {
                let error = AppError::from(CoreError::new(
                    code,
                    r"Секретная фраза sk-PRIVATE_KEY C:\PRIVATE_PROFILE\model.bin",
                    retryable,
                ));
                assert_eq!(
                    serde_json::to_value(&error).unwrap(),
                    serde_json::json!({"code":desktop_code,"params":{}})
                );
                assert!(!error.user_message().contains("PRIVATE"));
            }
        }
    }

    #[test]
    fn core_error_adapter_preserves_only_exact_authored_guidance() {
        use fono_types::{CoreError, ErrorCode};

        let guidance = "Сначала завершите текущую диктовку";
        assert_eq!(
            serde_json::to_value(AppError::from(CoreError::new(
                ErrorCode::Busy,
                guidance,
                true,
            )))
            .unwrap(),
            serde_json::json!({"code":"busy","params":{"detail":guidance}})
        );
        assert_eq!(
            serde_json::to_value(AppError::from(CoreError::new(
                ErrorCode::Busy,
                format!("{guidance}: PRIVATE_KEY"),
                true,
            )))
            .unwrap(),
            serde_json::json!({"code":"busy","params":{}})
        );
    }

    #[test]
    fn ipc_serialization_keeps_stable_codes_and_only_safe_parameters() {
        let encoded =
            serde_json::to_value(AppError::BackendUnavailable(RecognitionBackend::Cuda)).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({"code":"backend_unavailable","params":{"backend":"cuda"}})
        );
        assert!(AppError::BackendUnavailable(RecognitionBackend::Cuda)
            .user_message()
            .contains("CUDA"));
        let guidance = "Сначала завершите текущую диктовку";
        assert_eq!(
            serde_json::to_value(AppError::Busy(guidance.into())).unwrap(),
            serde_json::json!({"code":"busy","params":{"detail":guidance}})
        );
        assert_eq!(AppError::Busy(guidance.into()).user_message(), guidance);
    }

    #[test]
    fn source_errors_and_arbitrary_russian_details_never_reach_ipc() {
        for error in [
            AppError::Stt(r"model failed: C:\PRIVATE_PROFILE\model.bin sk-PRIVATE_KEY".into()),
            AppError::Config("Включите обработку текста PRIVATE_KEY".into()),
            AppError::Config("Секретная фраза пользователя PRIVATE_KEY".into()),
            AppError::Internal("Сначала завершите текущую диктовку PRIVATE_KEY".into()),
            AppError::Io(std::io::Error::other("PRIVATE_KEY /home/private/profile")),
        ] {
            let encoded = serde_json::to_string(&error).unwrap();
            assert!(!encoded.contains("PRIVATE"));
            assert!(!error.user_message().contains("PRIVATE"));
            assert!(error
                .user_message()
                .chars()
                .any(|value| ('А'..='я').contains(&value)));
            assert_eq!(
                serde_json::to_value(&error).unwrap()["params"],
                serde_json::json!({})
            );
        }
    }

    #[test]
    fn catalogue_is_valid_and_legacy_events_only_keep_exact_authored_guidance() {
        let catalog = catalog().expect("shared UI error catalogue parses");
        assert_eq!(catalog.messages.len(), 18);
        let guidance = "Поле ввода изменилось перед подготовкой буфера обмена";
        assert_eq!(
            safe_user_message(UserErrorCode::Injection, guidance),
            guidance
        );
        assert_ne!(
            safe_user_message(
                UserErrorCode::Injection,
                &format!("{guidance}: PRIVATE_KEY")
            ),
            guidance
        );
        assert!(!safe_user_message(
            UserErrorCode::Injection,
            &format!("{guidance}: PRIVATE_KEY")
        )
        .contains("PRIVATE"));
        for (error, expected) in [
            (
                AppError::BackendUnavailable(RecognitionBackend::Cuda),
                "Ускорение «CUDA» недоступно в этой сборке. Выберите другое ускорение в настройках.",
            ),
            (
                AppError::BackendStartFailed(RecognitionBackend::Cpu),
                "Не удалось запустить распознавание в режиме «Процессор». Проверьте модель и выбранное ускорение.",
            ),
        ] {
            assert_eq!(error.user_message(), expected);
            assert_eq!(safe_user_message(UserErrorCode::Stt, expected), expected);
            assert!(!safe_user_message(UserErrorCode::Stt, &format!("{expected} PRIVATE_KEY"))
                .contains("PRIVATE"));
        }
    }
}
