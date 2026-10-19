//! Bounded UTF-8 suffix passed through the existing Whisper token/context path.
pub(crate) const MAX_CONTEXT_BYTES: usize = 192;

pub(crate) fn bounded(text: &str) -> Option<&str> {
    let text = text.trim();
    if text.is_empty() || text.contains('\0') {
        return None;
    }
    let mut start = text.len().saturating_sub(MAX_CONTEXT_BYTES);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    Some(&text[start..])
}

#[cfg(test)]
mod tests {
    #[test]
    fn context_preserves_a_utf8_suffix_and_has_a_fixed_byte_bound() {
        for text in ["короткая фраза".into(), "ы🙂word ".repeat(100)] {
            let context = super::bounded(&text).unwrap();
            assert!(context.len() <= super::MAX_CONTEXT_BYTES);
            assert!(text.trim().ends_with(context));
            assert!(!context.contains('\u{fffd}'));
        }
        assert_eq!(super::bounded("  "), None);
        assert_eq!(super::bounded("bad\0context"), None);
    }
}
