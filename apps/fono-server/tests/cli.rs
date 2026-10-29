use std::path::PathBuf;
use std::process::{Command, Output};
use uuid::Uuid;

fn invocation(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fono-server"));
    command.args(args);
    // The parent process may have a server configuration of its own. Each CLI
    // test supplies only the Fono variables it explicitly wants to exercise.
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("FONO_")
        {
            command.env_remove(key);
        }
    }
    command
}

fn run_with_env(args: &[&str], environment: &[(&str, &str)]) -> Output {
    let mut command = invocation(args);
    for (key, value) in environment {
        command.env(key, value);
    }
    command.output().expect("run fono-server CLI")
}

fn run(args: &[&str]) -> Output {
    run_with_env(args, &[])
}

fn fixture_file(contents: &str) -> PathBuf {
    let directory = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = directory.join(format!(
        "fono-server-cli-{}-{}.toml",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    std::fs::write(&path, contents).expect("write CLI fixture");
    path
}

fn example_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/server.toml.example")
        .canonicalize()
        .expect("repository server configuration example")
}

fn fixture_directory() -> PathBuf {
    let directory = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = directory.join(format!(
        "fono-server-models-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&path).expect("create model fixture directory");
    path
}

#[test]
fn help_describes_the_server_commands() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in ["serve", "models", "token", "doctor", "config", "--version"] {
        assert!(stdout.contains(command), "missing {command} in help");
    }
    for (group, action) in [("models", "pull"), ("token", "revoke"), ("config", "check")] {
        let nested = run(&[group, "--help"]);
        assert!(nested.status.success());
        assert!(String::from_utf8(nested.stdout).unwrap().contains(action));
    }
}

#[test]
fn version_reports_product_api_and_worker_protocol() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let package = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../package.json"));
    let product_version = package
        .lines()
        .find(|line| line.trim_start().starts_with("\"version\":"))
        .and_then(|line| line.split('"').nth(3))
        .expect("product version in package.json");
    assert!(stdout.contains(&format!("fono-server {product_version}")));
    assert!(stdout.contains("api_version: 2"));
    assert!(stdout.contains(&format!(
        "worker_protocol: {}",
        fono_stt_protocol::PROTOCOL_VERSION
    )));
}

#[test]
fn config_check_accepts_valid_file_and_identifies_invalid_field() {
    let valid = fixture_file("[server]\nbind = \"127.0.0.1:17832\"\nmax_upload_mb = 100\n");
    let valid_path = valid.to_str().unwrap();
    let output = run(&["config", "check", "--config", valid_path]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("Configuration valid"));
    std::fs::remove_file(valid).unwrap();

    let invalid = fixture_file("[server]\nbind = \"not-an-address\"\n");
    let invalid_path = invalid.to_str().unwrap();
    let output = run(&["config", "check", "--config", invalid_path]);
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("server.bind"));
    std::fs::remove_file(invalid).unwrap();
}

#[test]
fn checked_in_server_example_passes_config_check() {
    let example = example_config();
    let output = run(&["config", "check", "--config", example.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn environment_overrides_invalid_file_fields_before_validation() {
    let config = fixture_file(
        "[server]\nbind = 'invalid'\n[queue]\nmax_queued = 0\n[[models.placement]]\nmodel = 'whisper:large_turbo'\ndevice = 'invalid'\n",
    );
    let output = run_with_env(
        &["config", "check", "--config", config.to_str().unwrap()],
        &[
            ("FONO_SERVER_BIND", "127.0.0.1:17832"),
            ("FONO_QUEUE_MAX_QUEUED", "4"),
            (
                "FONO_MODELS_PLACEMENT",
                "[{ model = 'whisper:large_turbo', device = 'cpu', residency = 'resident' }]",
            ),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_file(config).unwrap();
}

#[test]
fn invalid_environment_value_reports_field_without_echoing_value() {
    let example = example_config();
    let secret = "private-invalid-bind-value";
    let output = run_with_env(
        &["config", "check", "--config", example.to_str().unwrap()],
        &[("FONO_SERVER_BIND", secret)],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("server.bind"), "{stderr}");
    assert!(!stderr.contains(secret), "secret value was echoed");
}

#[test]
fn explicit_config_path_overrides_environment_path() {
    let example = example_config();
    let invalid = fixture_file("[server]\nbind = 'invalid'\n");
    let output = run_with_env(
        &["config", "check"],
        &[("FONO_CONFIG", example.to_str().unwrap())],
    );
    assert!(output.status.success());
    let output = run_with_env(
        &["config", "check", "--config", invalid.to_str().unwrap()],
        &[("FONO_CONFIG", example.to_str().unwrap())],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("server.bind"));
    std::fs::remove_file(invalid).unwrap();
}

#[test]
fn model_cli_lists_catalog_and_rejects_missing_or_corrupt_required_model() {
    let directory = fixture_directory();
    let config = fixture_file(&format!(
        "[models]\ndir = '{}'\ndefault = 'whisper:tiny'\npreload = ['whisper:tiny']\n",
        directory.display()
    ));
    let path = config.to_str().unwrap();
    let listed = run(&["models", "list", "--config", path]);
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let stdout = String::from_utf8(listed.stdout).unwrap();
    assert!(stdout.contains("whisper:tiny\tmissing"));
    assert!(stdout.contains("giga_am:v3_e2e_rnnt_int8"));

    let missing = run(&["models", "verify", "--config", path]);
    assert!(!missing.status.success());
    assert!(String::from_utf8(missing.stdout)
        .unwrap()
        .contains("whisper:tiny\tmissing"));

    let model = directory.join("ggml-tiny.bin");
    std::fs::write(&model, b"corrupt").unwrap();
    let corrupt = run(&["models", "verify", "--config", path]);
    assert!(!corrupt.status.success());
    assert!(String::from_utf8(corrupt.stdout)
        .unwrap()
        .contains("whisper:tiny\tinvalid"));

    let blocked = run(&["models", "remove", "whisper:tiny", "--config", path]);
    assert!(!blocked.status.success());
    assert!(model.exists());

    std::fs::remove_file(model).unwrap();
    std::fs::remove_file(config).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn unknown_model_id_and_config_reference_fail_before_any_download() {
    let directory = fixture_directory();
    let config = fixture_file(&format!("[models]\ndir = '{}'\n", directory.display()));
    let path = config.to_str().unwrap();
    let unknown = run(&["models", "pull", "whisper:../private", "--config", path]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8(unknown.stderr)
        .unwrap()
        .contains("Unknown managed model ID"));
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);

    let invalid = fixture_file("[models]\ndefault = 'private-model-id'\n");
    let checked = run(&["config", "check", "--config", invalid.to_str().unwrap()]);
    assert!(!checked.status.success());
    let stderr = String::from_utf8(checked.stderr).unwrap();
    assert!(stderr.contains("models.default"));
    assert!(!stderr.contains("private-model-id"));

    std::fs::remove_file(invalid).unwrap();
    std::fs::remove_file(config).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn token_cli_prints_secret_once_and_never_lists_or_persists_it() {
    let directory = fixture_directory();
    let token_file = directory.join("tokens");
    let config = fixture_file(&format!(
        "[server]\ntokens_file = '{}'\n",
        token_file.display()
    ));
    let path = config.to_str().unwrap();

    let created = run(&[
        "token",
        "create",
        "main-pc",
        "--scope",
        "recognize",
        "--config",
        path,
    ]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let secret = String::from_utf8(created.stdout).unwrap();
    let secret = secret.trim();
    assert!(secret.starts_with("fono_"));
    assert!(!secret.contains(char::is_whitespace));
    assert!(!String::from_utf8_lossy(&created.stderr).contains(secret));
    let stored = std::fs::read_to_string(&token_file).unwrap();
    assert!(stored.contains("main-pc\trecognize\t"));
    assert!(!stored.contains(secret));

    let duplicate = run(&["token", "create", "main-pc", "--config", path]);
    assert!(!duplicate.status.success());
    assert!(duplicate.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&duplicate.stderr).contains(secret));

    let listed = run(&["token", "list", "--config", path]);
    assert!(listed.status.success());
    let listing = String::from_utf8(listed.stdout).unwrap();
    assert!(listing.contains("main-pc\trecognize\t"));
    assert!(!listing.contains(secret));

    let revoked = run(&["token", "revoke", "main-pc", "--config", path]);
    assert!(revoked.status.success());
    assert!(!String::from_utf8_lossy(&revoked.stdout).contains(secret));
    let after = run(&["token", "list", "--config", path]);
    assert!(after.status.success());
    assert!(after.stdout.is_empty());

    std::fs::remove_file(config).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn doctor_reports_missing_dependencies_without_leaking_paths() {
    let config = fixture_file("[models]\ndir = '/missing-fono-doctor-models'\n[server]\ntokens_file = '/missing-fono-doctor-tokens'\n");
    let output = run(&["doctor", "--config", config.to_str().unwrap()]);
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ready"], false);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "tokens" && check["ok"] == false));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("/missing-fono"));
    std::fs::remove_file(config).unwrap();
}

#[test]
fn serve_checks_the_token_store_before_binding() {
    let missing_tokens = std::env::temp_dir().join(format!("fono-tokens-{}", Uuid::new_v4()));
    let config = fixture_file(&format!(
        "[server]\ntokens_file = '{}'\n",
        missing_tokens.to_string_lossy().replace('\\', "/")
    ));
    let output = run(&["serve", "--config", config.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("Token store"));
    std::fs::remove_file(config).unwrap();
}
