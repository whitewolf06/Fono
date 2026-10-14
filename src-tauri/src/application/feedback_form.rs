//! Opens only the build-configured public form. No report or diagnostics are sent.
use crate::error::{AppError, AppResult};
use reqwest::Url;

const CHANNEL: &str = include_str!("../../feedback-channel.json");

pub fn form_url(configured: &str, include_context: bool, allow_loopback: bool) -> AppResult<Url> {
    let invalid =
        || AppError::Config("Форма обратной связи пока не настроена для этой сборки.".into());
    if configured.is_empty()
        || configured.len() > 2048
        || configured
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return Err(invalid());
    }
    let mut url = Url::parse(configured).map_err(|_| invalid())?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    let prefix = "/feedback/submit/";
    let source = url.path().strip_prefix(prefix).ok_or_else(invalid)?;
    let canonical_source = uuid::Uuid::parse_str(source)
        .ok()
        .is_some_and(|id| id.to_string() == source);
    if (url.scheme() != "https" && !(allow_loopback && url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !canonical_source
        || url.as_str() != configured
    {
        return Err(invalid());
    }
    if include_context {
        let fragment = format!(
            "version={}&platform=windows&route=%2Fsettings%2Fdiagnostics",
            env!("CARGO_PKG_VERSION").replace('+', "%2B")
        );
        url.set_fragment(Some(&fragment));
    }
    Ok(url)
}

pub fn open(include_context: bool) -> AppResult<()> {
    let configured = serde_json::from_str::<serde_json::Value>(CHANNEL)
        .ok()
        .and_then(|config| config["publicFormUrl"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let url = form_url(&configured, include_context, cfg!(debug_assertions))?;
    crate::application::project_site::open_url(url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORM: &str =
        "https://feedback.example/feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea";

    #[test]
    fn default_config_is_disabled() {
        assert!(form_url("", false, false).is_err());
    }

    #[test]
    fn valid_url_has_no_context_by_default() {
        assert_eq!(form_url(FORM, false, false).unwrap().as_str(), FORM);
    }

    #[test]
    fn explicit_context_is_allowlisted_and_in_fragment_only() {
        let url = form_url(FORM, true, false).unwrap();
        assert!(url.query().is_none());
        let fragment = url.fragment().unwrap();
        assert!(fragment.starts_with("version="));
        assert!(fragment.contains("&platform=windows&route=%2Fsettings%2Fdiagnostics"));
        assert_eq!(fragment.matches('&').count(), 2);
    }

    #[test]
    fn rejects_credentials_other_paths_query_fragment_and_non_http_schemes() {
        for value in [
            "javascript:alert(1)",
            "file:///feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea",
            "https://user:secret@feedback.example/feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea",
            "https://feedback.example/feedback/submit/not-a-uuid",
            "https://feedback.example/feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea?token=secret",
            "https://feedback.example/feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea#private",
            "https://feedback.example/ignored/../feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea",
        ] {
            assert!(form_url(value, false, true).is_err(), "{value}");
        }
    }

    #[test]
    fn http_is_only_allowed_for_explicit_loopback_debug() {
        let local = FORM.replace("https://feedback.example", "http://127.0.0.1:5380");
        assert!(form_url(&local, false, true).is_ok());
        assert!(form_url(&local, false, false).is_err());
        assert!(form_url(&FORM.replace("https:", "http:"), false, true).is_err());
    }
}
