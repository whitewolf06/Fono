use super::*;
use std::sync::atomic::AtomicUsize;

fn key(seed: u8) -> String {
    let mut bytes = Vec::from(11u32.to_be_bytes());
    bytes.extend_from_slice(b"ssh-ed25519");
    bytes.extend_from_slice(&32u32.to_be_bytes());
    bytes.extend_from_slice(&[seed; 32]);
    format!("ssh-ed25519 {}", general_purpose::STANDARD.encode(bytes))
}

fn settings() -> RemoteSshSettings {
    RemoteSshSettings {
        host: "server.example".into(),
        port: 22,
        user: "fono-user".into(),
        identity_file: None,
        server_port: 17832,
        host_key: key(7),
    }
}

#[test]
fn configuration_rejects_option_injection_and_unpinned_or_malformed_host_keys() {
    let valid = settings();
    assert_eq!(validate(&valid), Ok(()));
    for host in [
        "-oProxyCommand=anything",
        "user@host",
        "host:22",
        "host\nother",
        "a b",
        "/tmp/socket",
    ] {
        let mut value = valid.clone();
        value.host = host.into();
        assert_eq!(validate(&value), Err(SshError::InvalidConfiguration));
    }
    for host in ["127.0.0.1", "::1", "server.example", "machine-name"] {
        let mut value = valid.clone();
        value.host = host.into();
        assert_eq!(validate(&value), Ok(()));
    }
    let mut value = valid.clone();
    value.user = "-oProxyCommand=anything".into();
    assert_eq!(validate(&value), Err(SshError::InvalidConfiguration));
    value = valid.clone();
    value.identity_file = Some(PathBuf::from("relative-key"));
    assert_eq!(validate(&value), Err(SshError::InvalidConfiguration));
    value = valid.clone();
    value.host_key.clear();
    assert_eq!(validate(&value), Err(SshError::HostKeyRequired));
    for bad in [
        "ssh-ed25519 AAAA",
        "ssh-rsa AAAA",
        "ssh-ed25519 AAAA comment",
        "ssh-ed25519 AAAA\n",
    ] {
        value.host_key = bad.into();
        assert_eq!(validate(&value), Err(SshError::InvalidConfiguration));
    }
    let mut blob = general_purpose::STANDARD
        .decode(key(7).split_once(' ').unwrap().1)
        .unwrap();
    blob[18] = 31;
    value.host_key = format!("ssh-ed25519 {}", general_purpose::STANDARD.encode(blob));
    assert_eq!(validate(&value), Err(SshError::InvalidConfiguration));
}

#[test]
fn fingerprint_is_standard_sha256_and_requires_exact_ed25519_wire_layout() {
    let parsed = parse_host_key(&key(7)).unwrap();
    assert_eq!(parsed.host_key, key(7));
    let blob = general_purpose::STANDARD
        .decode(parsed.host_key.split_once(' ').unwrap().1)
        .unwrap();
    assert_eq!(
        parsed.fingerprint,
        format!(
            "SHA256:{}",
            general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(blob))
        )
    );
    assert!(!parsed.fingerprint.ends_with('='));
    assert_ne!(
        parsed.fingerprint,
        parse_host_key(&key(8)).unwrap().fingerprint
    );
}

#[test]
fn command_has_only_pinned_noninteractive_stdio_forwarding_and_quotes_host_file() {
    let mut configured = settings();
    let host_file = Arc::new(PrivateHostFile::new(&configured.host_key).unwrap());
    configured.identity_file = Some(std::env::temp_dir().join("test key with spaces"));
    let process = SshProcess {
        executable: PathBuf::from("not-executed-ssh"),
        settings: configured,
        host_file,
    };
    let command = process.command();
    let args: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    for argument in [
        "StrictHostKeyChecking=yes",
        "BatchMode=yes",
        "PasswordAuthentication=no",
        "ForwardAgent=no",
        "IdentitiesOnly=yes",
        "ClearAllForwardings=yes",
        "HostKeyAlgorithms=ssh-ed25519",
        "ExitOnForwardFailure=yes",
        "HostKeyAlias=fono-pinned-server",
        "127.0.0.1:17832",
    ] {
        assert!(
            args.iter().any(|value| value == argument),
            "missing fixed SSH argument"
        );
    }
    assert!(!args.iter().any(|value| value == "-L" || value == "-D"));
    assert_eq!(&args[..4], &["-F", null_device(), "-T", "-W"]);
    let host_option = args
        .iter()
        .find(|arg| arg.starts_with("UserKnownHostsFile="))
        .unwrap();
    assert!(host_option.starts_with("UserKnownHostsFile=\""));
    assert!(host_option.ends_with('"'));
    assert_eq!(args.last().unwrap(), "server.example");
    let content = fs::read_to_string(&process.host_file.path).unwrap();
    assert_eq!(content, format!("{HOST_ALIAS} {}\n", key(7)));
}

struct Fixture {
    directory: PathBuf,
    executable: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let target = self.directory.canonicalize().unwrap();
        if target.parent() == Some(parent.as_path())
            && target
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("fono-ssh-fixture-")
            && fs::symlink_metadata(&self.directory)
                .is_ok_and(|meta| !meta.file_type().is_symlink())
        {
            let _ = fs::remove_dir_all(&target);
        }
    }
}

fn fixture() -> Arc<Fixture> {
    static FIXTURE: OnceLock<Mutex<std::sync::Weak<Fixture>>> = OnceLock::new();
    let mut cached = FIXTURE
        .get_or_init(|| Mutex::new(std::sync::Weak::new()))
        .lock();
    if let Some(fixture) = cached.upgrade() {
        return fixture;
    }
    let fixture = Arc::new({
        let directory =
            std::env::temp_dir().join(format!("fono-ssh-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let source = directory.join("stdio.rs");
        let executable = directory.join(if cfg!(windows) { "stdio.exe" } else { "stdio" });
        fs::write(
            &source,
            r#"
use std::{io::{self, Read, Write}, thread, time::Duration};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "echo" => { io::copy(&mut io::stdin(), &mut io::stdout()).unwrap(); },
        "half" => {
            let mut bytes = Vec::new();
            io::stdin().read_to_end(&mut bytes).unwrap();
            io::stdout().write_all(b"response:").unwrap();
            io::stdout().write_all(&bytes).unwrap();
            io::stdout().write_all(&vec![42; 192 * 1024]).unwrap();
        },
        "print" => { io::stdout().write_all(args[2].as_bytes()).unwrap(); },
        "blocked" => { loop { thread::sleep(Duration::from_secs(1)); } },
        _ => std::process::exit(2),
    }
}
"#,
        )
        .unwrap();
        let compiled = Command::new("rustc")
            .args(["--edition=2021", "-C", "debuginfo=0"])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "SSH fixture compile failed: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        Fixture {
            directory,
            executable,
        }
    });
    *cached = Arc::downgrade(&fixture);
    fixture
}

fn child_command(fixture: &Fixture, mode: &str) -> Command {
    let mut command = Command::new(&fixture.executable);
    hidden(&mut command);
    command
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    command
}

fn factory(fixture: Arc<Fixture>, mode: &'static str, counter: Arc<AtomicUsize>) -> ProcessFactory {
    // Resolve the compiled public fixture before the accept thread runs.
    Arc::new(move || {
        counter.fetch_add(1, Ordering::AcqRel);
        let mut command = Command::new(&fixture.executable);
        hidden(&mut command);
        command
            .arg(mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        command
    })
}

fn socket(tunnel: &Tunnel) -> TcpStream {
    let socket = TcpStream::connect(tunnel.url().strip_prefix("http://").unwrap()).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    socket
}

fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "SSH fixture did not reach expected state"
        );
        thread::sleep(TICK);
    }
}

#[test]
fn owned_child_stop_reaps_exact_process_without_stopping_another_fixture() {
    let fixture = fixture();
    let mut first = OwnedChild::spawn(&mut child_command(&fixture, "blocked")).unwrap();
    let mut other = OwnedChild::spawn(&mut child_command(&fixture, "blocked")).unwrap();
    first.stop();
    assert!(first.child.try_wait().unwrap().is_some());
    assert!(other.child.try_wait().unwrap().is_none());
}

#[test]
fn actual_stdio_process_preserves_half_close_and_drains_the_final_response() {
    let counter = Arc::new(AtomicUsize::new(0));
    let tunnel = Tunnel::start(factory(fixture(), "half", counter.clone())).unwrap();
    let mut socket = socket(&tunnel);
    let request = [0, 255, 1, 128, 44, 0, 23];
    socket.write_all(&request).unwrap();
    socket.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).unwrap();
    let mut expected = b"response:".to_vec();
    expected.extend_from_slice(&request);
    expected.extend_from_slice(&vec![42; 192 * 1024]);
    assert_eq!(response, expected);
    assert_eq!(counter.load(Ordering::Acquire), 1);
}

#[test]
fn completed_connections_are_pruned_and_can_exceed_the_concurrent_cap_over_time() {
    let counter = Arc::new(AtomicUsize::new(0));
    let tunnel = Tunnel::start(factory(fixture(), "echo", counter.clone())).unwrap();
    for index in 0..MAX_CONNECTIONS * 2 {
        let mut socket = socket(&tunnel);
        socket.write_all(&[index as u8]).unwrap();
        socket.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).unwrap();
        assert_eq!(response, [index as u8]);
    }
    assert_eq!(counter.load(Ordering::Acquire), MAX_CONNECTIONS * 2);
}

#[test]
fn concurrent_cap_and_drop_bound_own_blocked_children_and_close_the_listener() {
    let fixture = fixture();
    let mut other = OwnedChild::spawn(&mut child_command(&fixture, "blocked")).unwrap();
    let counter = Arc::new(AtomicUsize::new(0));
    let tunnel = Tunnel::start(factory(fixture.clone(), "blocked", counter.clone())).unwrap();
    let address = tunnel.url().strip_prefix("http://").unwrap().to_owned();
    let sockets: Vec<_> = (0..MAX_CONNECTIONS).map(|_| socket(&tunnel)).collect();
    until(|| counter.load(Ordering::Acquire) == MAX_CONNECTIONS);
    let mut rejected = socket(&tunnel);
    assert!(matches!(rejected.read(&mut [0]), Ok(0) | Err(_)));
    assert_eq!(counter.load(Ordering::Acquire), MAX_CONNECTIONS);
    let started = Instant::now();
    drop(tunnel);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "owned tunnel teardown exceeded its bound"
    );
    assert!(TcpStream::connect(address).is_err());
    for mut socket in sockets {
        assert!(matches!(socket.read(&mut [0]), Ok(0) | Err(_)));
    }
    assert!(other.child.try_wait().unwrap().is_none());
}

#[test]
fn keyscan_is_bounded_and_never_accepts_another_host_or_malformed_blob() {
    let fixture = fixture();
    let valid = format!("server.example {}\n", key(7));
    let mut command = child_command(&fixture, "print");
    command.arg(&valid);
    assert_eq!(
        capture_host_key(command, "server.example", 22).unwrap(),
        parse_host_key(&key(7)).unwrap()
    );
    for output in [
        format!("other.example {}\n", key(7)),
        "server.example ssh-ed25519 AAAA\n".into(),
        format!("{valid}server.example {}\n", key(8)),
        "x".repeat(MAX_KEYSCAN_BYTES + 1),
    ] {
        let mut command = child_command(&fixture, "print");
        command.arg(output);
        assert_eq!(
            capture_host_key(command, "server.example", 22),
            Err(SshError::ConnectionFailed)
        );
    }
}
