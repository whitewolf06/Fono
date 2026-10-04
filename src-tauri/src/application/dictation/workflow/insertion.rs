//! Native insertion checks the original destination at every send boundary.
use super::{Session, TextTarget};
use crate::{
    application::updates::activity,
    error::{AppError, AppResult},
    injection::target::AppendResult,
};

pub(super) async fn insert(
    session: &Session,
    target: Option<&TextTarget>,
    text: &str,
) -> AppResult<()> {
    let target = target
        .filter(|target| crate::injection::target::matches(target))
        .cloned()
        .ok_or_else(|| {
            AppError::Injection(
                "Поле для вставки изменилось или недоступно. Скопируйте текст из Fono или начните новую диктовку в нужном поле".into(),
            )
        })?;
    let activity = activity::lease()?;
    let text = text.to_owned();
    let expected = text.len();
    let cancellation = session.cancellation.clone();
    let mode = session.settings.injection_mode;
    let output = tokio::task::spawn_blocking(move || {
        let _activity = activity;
        crate::injection::target::append(&target, &text, mode, &cancellation)
    })
    .await
    .map_err(|error| AppError::Injection(format!("Вставка прервана: {error}")))??;
    validate_completion(output, expected)
}

fn validate_completion(output: AppendResult, expected: usize) -> AppResult<()> {
    if !output.paused && !output.uncertain && output.bytes == expected {
        return Ok(());
    }
    Err(AppError::Injection("Вставка остановлена или отправлена частично. Проверьте поле; полный текст доступен в Fono для копирования".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_paused_and_uncertain_sends_never_claim_success() {
        for output in [
            AppendResult {
                bytes: 2,
                paused: true,
                uncertain: false,
            },
            AppendResult {
                bytes: 4,
                paused: false,
                uncertain: true,
            },
            AppendResult {
                bytes: 2,
                paused: false,
                uncertain: false,
            },
        ] {
            assert!(validate_completion(output, 4).is_err());
        }
        assert!(validate_completion(
            AppendResult {
                bytes: 4,
                paused: false,
                uncertain: false
            },
            4
        )
        .is_ok());
    }
}
