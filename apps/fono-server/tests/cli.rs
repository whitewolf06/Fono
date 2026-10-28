use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fono-server"))
        .args(args)
        .output()
        .expect("run fono-server CLI")
}

fn fixture_file(contents: &str) -> PathBuf {
    let directory = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = directory.join(format!(
        "fono-server-cli-{}-{nonce}.toml",
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("write CLI fixture");
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
fn version_reports_product_and_worker_protocol_without_claiming_a_running_api() {
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
    assert!(stdout.contains("api_version: 2 (planned"));
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
fn pending_runtime_commands_fail_explicitly() {
    for args in [
        &["serve"][..],
        &["models", "list"],
        &["models", "pull", "whisper:base"],
        &["token", "create", "client"],
        &["doctor"],
    ] {
        let output = run(args);
        assert!(!output.status.success(), "unexpected success: {args:?}");
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("not available"));
    }
}
