//! Типы ошибок приложения.

use thiserror::Error;

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

    #[error("внутренняя ошибка: {0}")]
    Internal(String),
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
        s.serialize_str(self.to_string().as_ref())
    }
}

pub type AppResult<T> = Result<T, AppError>;
