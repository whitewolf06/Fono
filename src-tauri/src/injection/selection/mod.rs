//! Explicit selection reads only. Ordinary target guards never call this module.
use super::target::TextTarget;
use crate::error::{AppError, AppResult};
use fono_core::OperationCancellation;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[cfg(windows)]
mod windows;

pub(crate) const MAX_UTF16: usize = 32_768;
const MAX_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SelectionSource {
    UiAutomation,
    Clipboard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectionAnchor {
    UiAutomation { operation: u64 },
    ClassicEdit { start: u32, end: u32 },
}

#[derive(Clone)]
pub(crate) struct CapturedSelection {
    pub(crate) text: String,
    pub(crate) source: SelectionSource,
    pub(crate) target: TextTarget,
    pub(crate) anchor: SelectionAnchor,
    pub(crate) digest: [u8; 32],
}
impl CapturedSelection {
    pub(crate) fn belongs_to(&self, operation: u64) -> bool {
        let anchor = match (&self.anchor, self.source) {
            (SelectionAnchor::UiAutomation { operation: owner }, SelectionSource::UiAutomation) => {
                *owner == operation
            }
            (SelectionAnchor::ClassicEdit { start, end }, SelectionSource::Clipboard) => {
                start < end && end - start <= MAX_UTF16 as u32
            }
            _ => false,
        };
        anchor
            && self.target.window != 0
            && self.target.focus != 0
            && self.target.process != 0
            && !self.target.runtime_id.is_empty()
            && self.digest == <[u8; 32]>::from(Sha256::digest(self.text.as_bytes()))
    }
}

pub(crate) fn validate_text(text: String) -> AppResult<String> {
    if text.is_empty() {
        return Err(error(
            "Нет выделенного текста. Выделите фрагмент и повторите действие.",
        ));
    }
    if text.len() > MAX_BYTES || text.encode_utf16().count() > MAX_UTF16 {
        return Err(error(
            "Выделение слишком большое. Выберите фрагмент до 32 тысяч символов.",
        ));
    }
    if text.contains('\0') {
        return Err(error("Выделение содержит неподдерживаемые символы."));
    }
    Ok(text)
}

pub(crate) fn captured(
    text: String,
    source: SelectionSource,
    target: TextTarget,
    anchor: SelectionAnchor,
) -> AppResult<CapturedSelection> {
    let text = validate_text(text)?;
    let digest = Sha256::digest(text.as_bytes()).into();
    Ok(CapturedSelection {
        text,
        source,
        target,
        anchor,
        digest,
    })
}

pub(crate) fn error(message: &str) -> AppError {
    AppError::Injection(message.into())
}

pub(crate) fn capture(
    operation: u64,
    cancel: OperationCancellation,
    fallback: bool,
    lease: crate::application::updates::activity::ActivityLease<'static>,
) -> AppResult<CapturedSelection> {
    #[cfg(windows)]
    {
        windows::capture(operation, cancel, fallback, lease)
    }
    #[cfg(not(windows))]
    {
        let _ = (operation, cancel, fallback, lease);
        Err(error("Получение выделения доступно только в Windows."))
    }
}

pub(crate) fn replace(
    operation: u64,
    original: CapturedSelection,
    text: &str,
    mode: crate::types::InjectionMode,
    cancel: OperationCancellation,
) -> AppResult<super::target::AppendResult> {
    if !original.belongs_to(operation) {
        return Err(error(
            "Поле или выделение изменилось. Выделите фрагмент заново.",
        ));
    }
    validate_text(text.to_owned())?;
    #[cfg(windows)]
    {
        windows::replace(operation, original, text, mode, cancel)
    }
    #[cfg(not(windows))]
    {
        let _ = (original, mode, cancel);
        Err(error("Вставка поддерживается только в Windows"))
    }
}

pub(crate) fn release(operation: u64) {
    #[cfg(windows)]
    windows::release(operation);
    #[cfg(not(windows))]
    let _ = operation;
}

pub(crate) fn shutdown() {
    #[cfg(windows)]
    windows::shutdown();
}

#[cfg(windows)]
pub(crate) use windows::matches_classic;

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_settings_never_enable_selection_reads_or_clipboard_copy() {
        let s: crate::types::Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.selection_capture_enabled);
        assert!(!s.selection_clipboard_fallback);
        assert_eq!(s.selection_hotkey, "Ctrl+Alt+Space");
        let roundtrip: crate::types::Settings =
            serde_json::from_value(serde_json::to_value(s).unwrap()).unwrap();
        assert!(!roundtrip.selection_capture_enabled);
    }
    #[test]
    fn changed_payload_or_foreign_operation_cannot_reuse_a_selection_proof() {
        let target = TextTarget {
            window: 1,
            focus: 2,
            process: 3,
            runtime_id: vec![4],
        };
        let mut selection = captured(
            "text".into(),
            SelectionSource::UiAutomation,
            target,
            SelectionAnchor::UiAutomation { operation: 7 },
        )
        .unwrap();
        assert!(selection.belongs_to(7));
        assert!(!selection.belongs_to(8));
        selection.text = "changed".into();
        assert!(!selection.belongs_to(7));
    }
    use super::*;
    #[test]
    fn empty_and_oversized_selections_never_become_a_document_read() {
        assert!(validate_text(String::new()).is_err());
        assert_eq!(validate_text(" ".into()).unwrap(), " ");
        assert!(validate_text("a".repeat(MAX_UTF16 + 1)).is_err());
        assert!(validate_text("я".repeat(MAX_UTF16)).is_ok());
        assert!(validate_text("😀".repeat(MAX_UTF16 / 2 + 1)).is_err());
        assert!(validate_text("private\0tail".into()).is_err());
    }
    #[test]
    fn identity_pins_field_range_and_exact_selected_content() {
        let target = TextTarget {
            window: 1,
            focus: 2,
            process: 3,
            runtime_id: vec![4],
        };
        let anchor = SelectionAnchor::ClassicEdit { start: 5, end: 9 };
        let a = captured(
            "test".into(),
            SelectionSource::Clipboard,
            target.clone(),
            anchor.clone(),
        )
        .unwrap();
        let b = captured(
            "Test".into(),
            SelectionSource::Clipboard,
            target.clone(),
            anchor.clone(),
        )
        .unwrap();
        assert_eq!(a.target, target);
        assert_eq!(a.anchor, anchor);
        assert_ne!(a.digest, b.digest);
    }
}
