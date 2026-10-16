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
