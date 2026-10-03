use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Verifies binaries, rather than trusting a sidecar written by an older build.
/// Hello loads no model and invokes no GPU inference.
pub fn validate(manifest: &Path, directory: &Path) -> Result<(), String> {
    let source = fs::read_to_string(manifest.join("crates/fono-stt-protocol/src/lib.rs"))
        .map_err(|error| error.to_string())?;
    let version: u16 = source
        .lines()
        .find(|line| line.starts_with("pub const PROTOCOL_VERSION:"))
        .and_then(|line| line.split('=').nth(1))
        .map(|value| value.trim().trim_end_matches(';'))
        .ok_or("cannot read STT protocol version")?
        .parse()
        .map_err(|error| format!("protocol version: {error}"))?;
    for backend in ["cuda", "vulkan"] {
        handshake(
            &directory.join(format!("fono-stt-{backend}-worker.exe")),
            directory,
            backend,
            version,
        )?;
    }
    Ok(())
}

fn handshake(path: &Path, directory: &Path, backend: &str, version: u16) -> Result<(), String> {
    let mut command = Command::new(path);
    command
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("{backend} worker start: {error}"))?;
    let result = (|| {
        let stdout = child.stdout.take().ok_or("worker stdout unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout)
                .take(1024 * 1024 + 1)
                .read_line(&mut line)
                .map_err(|error| error.to_string())
                .and_then(|_| {
                    if line.len() > 1024 * 1024 {
                        Err("worker hello response exceeds limit".into())
                    } else {
                        Ok(line)
                    }
                });
            let _ = sender.send(result);
        });
        let sent = child.stdin.as_mut().ok_or("worker stdin unavailable").and_then(|stdin| {
            let request = serde_json::json!({"type":"hello", "protocol_version":version, "request_id":"release-verification"});
            writeln!(stdin, "{request}").and_then(|_| stdin.flush()).map_err(|_| "worker hello write failed")
        });
        let response = if sent.is_ok() {
            receiver
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| format!("{backend} worker hello timed out after 5 seconds"))
                .and_then(|result| result)
        } else {
            Err("worker hello write failed".into())
        };
        // Always release the pipe reader, including a hung/incompatible binary.
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        let response: serde_json::Value = serde_json::from_str(&response?)
            .map_err(|error| format!("{backend} hello JSON: {error}"))?;
        if response["type"] != "ready"
            || response["request_id"] != "release-verification"
            || response["protocol_version"] != version
            || response["backend"] != backend
        {
            return Err(format!(
                "{backend} binary does not implement protocol {version}"
            ));
        }
        let capabilities = &response["capabilities"];
        for name in [
            "supports_window",
            "supports_cancel",
            "supports_token_timestamps",
            "supports_health",
            "supports_shutdown",
        ] {
            if capabilities[name] != true {
                return Err(format!("{backend} binary lacks {name}"));
            }
        }
        if capabilities["protocol_version"] != version
            || capabilities["maximum_request_bytes"].as_u64().unwrap_or(0) < 16777216
            || capabilities["maximum_response_bytes"].as_u64().unwrap_or(0) < 1048576
        {
            return Err(format!(
                "{backend} binary has incompatible transport limits"
            ));
        }
        Ok(())
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}
