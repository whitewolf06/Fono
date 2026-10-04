use crate::error::{AppError, AppResult};

/// Restore only after detector reliability is accepted with real voice checks.
pub const WAKE_WORD_AVAILABLE: bool = false;
pub const WAKE_WORD_UNAVAILABLE: &str =
    "Пробуждение голосом временно недоступно: доработаем качество распознавания. Используйте кнопку или горячую клавишу.";

pub fn ensure_wake_available() -> AppResult<()> {
    if WAKE_WORD_AVAILABLE {
        Ok(())
    } else {
        Err(AppError::Config(WAKE_WORD_UNAVAILABLE.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_wake_rejects_activation_before_any_capture() {
        assert!(ensure_wake_available().is_err());
    }
}
