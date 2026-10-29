use std::path::Path;

use super::*;

pub(super) fn config() -> ServerConfig {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/server.toml.example");
    let mut config = crate::config::load_and_validate_with_env(&path, std::iter::empty()).unwrap();
    config.server.bind = "127.0.0.1:0".parse().unwrap();
    config
}

#[test]
fn port_conflict_is_reported_without_disturbing_the_existing_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = config();
    config.server.bind = address;
    let mut report = Report::new();
    inspect_network(&config, &mut report);
    let check = report
        .checks
        .iter()
        .find(|check| check.name == "listener")
        .unwrap();
    assert!(!check.ok && check.required);
    assert!(check.remediation.unwrap().contains("existing listener"));
    assert_eq!(listener.local_addr().unwrap(), address);
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains(&address.to_string()));
}

#[test]
fn protected_lan_still_reports_plaintext_exposure_as_a_warning() {
    let mut config = config();
    config.server.bind = "0.0.0.0:0".parse().unwrap();
    config.server.trusted_private_network = true;
    config.server.allowed_hosts = vec!["private-doctor-host.invalid".into()];
    let mut report = Report::new();
    inspect_network(&config, &mut report);
    assert!(
        report
            .checks
            .iter()
            .find(|check| check.name == "network_policy")
            .unwrap()
            .ok
    );
    let exposure = report
        .checks
        .iter()
        .find(|check| check.name == "network_exposure")
        .unwrap();
    assert!(!exposure.ok && !exposure.required);
    assert!(report.required_checks_pass());
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("private-doctor-host"));
}

#[test]
fn failed_preflight_skips_recognition_and_redacts_configuration_values() {
    let mut config = config();
    config.models.default = "whisper:tiny".into();
    config.models.preload = vec![config.models.default.clone()];
    config.models.dir =
        std::env::temp_dir().join(format!("private-doctor-models-{}", uuid::Uuid::new_v4()));
    config.server.tokens_file = config.models.dir.join("private-token-name");
    let report = inspect(
        &config,
        Path::new("unreadable-secret-config"),
        Options::default(),
    );
    assert!(!report.ready);
    assert_eq!(report.self_test.status, "skipped");
    assert!(report
        .checks
        .iter()
        .filter(|check| check.required && !check.ok)
        .all(|check| check.remediation.is_some_and(|hint| !hint.is_empty())));
    let encoded = serde_json::to_string(&report).unwrap();
    for private in [
        "private-doctor-models",
        "private-token-name",
        "unreadable-secret-config",
    ] {
        assert!(!encoded.contains(private));
    }
    let model = report
        .models
        .iter()
        .find(|model| model.id == "whisper:tiny")
        .unwrap();
    assert!(model.required);
    assert_eq!(model.state, "missing");
}

#[test]
fn skipped_probe_is_explicit_and_never_claims_speech_passed() {
    let report = inspect(
        &config(),
        Path::new("unused"),
        Options {
            skip_self_test: true,
            ..Options::default()
        },
    );
    assert_eq!(report.self_test.status, "skipped");
    let check = report
        .checks
        .iter()
        .find(|check| check.name == "self_test")
        .unwrap();
    assert!(!check.required && !check.ok);
}
