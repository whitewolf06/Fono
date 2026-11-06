//! App-owned loopback relay into pinned OpenSSH stdio channels. There is no
//! released-port race: Rust retains the listener and every accepted socket.
use crate::types::RemoteSshSettings;
use base64::{engine::general_purpose, Engine};
use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Read, Write},
    net::{IpAddr, Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_CONNECTIONS: usize = 8;
const MAX_KEYSCAN_BYTES: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const IDLE_LIMIT: Duration = Duration::from_secs(10 * 60);
const TICK: Duration = Duration::from_millis(20);
const IO_TICK: Duration = Duration::from_millis(100);
const HOST_ALIAS: &str = "fono-pinned-server";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SshError {
    InvalidConfiguration,
    ClientMissing,
    IdentityMissing,
    HostKeyRequired,
    ConnectionFailed,
    Busy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct HostKey {
    pub host_key: String,
    pub fingerprint: String,
}

fn host_valid(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.bytes().any(|byte| !byte.is_ascii()) {
        return false;
    }
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    host.split('.').all(|part| {
        !part.is_empty()
            && part.len() <= 63
            && part.as_bytes()[0].is_ascii_alphanumeric()
            && part.as_bytes()[part.len() - 1].is_ascii_alphanumeric()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn parse_host_key(value: &str) -> Result<HostKey, SshError> {
    if value.trim().is_empty() {
        return Err(SshError::HostKeyRequired);
    }
    if value.len() > 256 || value.contains(['\r', '\n', '\0']) {
        return Err(SshError::InvalidConfiguration);
    }
    let mut fields = value.split_ascii_whitespace();
    if fields.next() != Some("ssh-ed25519") {
        return Err(SshError::InvalidConfiguration);
    }
    let encoded = fields.next().ok_or(SshError::InvalidConfiguration)?;
    if fields.next().is_some() {
        return Err(SshError::InvalidConfiguration);
    }
    let decoded = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| SshError::InvalidConfiguration)?;
    // RFC 4253 string("ssh-ed25519"), string(exactly 32 public-key bytes).
    if decoded.len() != 51
        || decoded[..4] != 11u32.to_be_bytes()
        || &decoded[4..15] != b"ssh-ed25519"
        || decoded[15..19] != 32u32.to_be_bytes()
    {
        return Err(SshError::InvalidConfiguration);
    }
    Ok(HostKey {
        host_key: format!("ssh-ed25519 {}", general_purpose::STANDARD.encode(&decoded)),
        fingerprint: format!(
            "SHA256:{}",
            general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(&decoded))
        ),
    })
}

/// Pure syntax validation. File access and process discovery belong to resolve.
pub(crate) fn validate(settings: &RemoteSshSettings) -> Result<(), SshError> {
    if !host_valid(&settings.host)
        || settings.port == 0
        || settings.server_port == 0
        || settings.user.is_empty()
        || settings.user.len() > 64
        || !settings.user.as_bytes()[0].is_ascii_alphanumeric()
        || !settings
            .user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        || settings.identity_file.as_ref().is_some_and(|path| {
            !path.is_absolute()
                || path.as_os_str().is_empty()
                || path.to_string_lossy().chars().any(char::is_control)
        })
    {
        return Err(SshError::InvalidConfiguration);
    }
    parse_host_key(&settings.host_key)?;
    Ok(())
}

fn binary(name: &str) -> Result<PathBuf, SshError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
        let mut directory = vec![0u16; 32_768];
        let count = unsafe { GetSystemDirectoryW(Some(&mut directory)) } as usize;
        if count == 0 || count >= directory.len() {
            return Err(SshError::ClientMissing);
        }
        let file = PathBuf::from(std::ffi::OsString::from_wide(&directory[..count]))
            .join("OpenSSH")
            .join(format!("{name}.exe"));
        if file.is_file() {
            Ok(file)
        } else {
            Err(SshError::ClientMissing)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Err(SshError::ClientMissing)
    }
}

fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.stdin(Stdio::null()).stderr(Stdio::null());
}

/// Only the exact spawned process belongs to this handle. A job also closes it
/// if Fono is terminated before normal shutdown can run.
struct OwnedChild {
    child: Child,
    #[cfg(windows)]
    _job: Job,
}

impl OwnedChild {
    fn spawn(command: &mut Command) -> Result<Self, SshError> {
        let child = command.spawn().map_err(|_| SshError::ConnectionFailed)?;
        #[cfg(windows)]
        let job = match Job::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            child,
            #[cfg(windows)]
            _job: job,
        })
    }

    fn stop(&mut self) {
        #[cfg(windows)]
        self._job.terminate();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
struct Job(windows::Win32::Foundation::HANDLE);

// A job handle is not tied to an acquiring thread, unlike a Win32 mutex.
// It has exactly one owner and is closed only after its child is reaped.
#[cfg(windows)]
unsafe impl Send for Job {}

#[cfg(windows)]
impl Job {
    fn terminate(&self) {
        // All members were created for this relay. Killing the complete owned
        // job also releases inherited pipe handles before reader joins.
        unsafe {
            let _ = windows::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }

    fn attach(child: &Child) -> Result<Self, SshError> {
        use std::os::windows::io::AsRawHandle;
        use windows::{
            core::PCWSTR,
            Win32::{Foundation::HANDLE, System::JobObjects::*},
        };
        unsafe {
            let job = Self(
                CreateJobObjectW(None, PCWSTR::null()).map_err(|_| SshError::ConnectionFailed)?,
            );
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&limits) as u32,
            )
            .map_err(|_| SshError::ConnectionFailed)?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()))
                .map_err(|_| SshError::ConnectionFailed)?;
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub(crate) fn probe_host_key(host: &str, port: u16) -> Result<HostKey, SshError> {
    static PROBE: Mutex<()> = Mutex::new(());
    let _probe = PROBE.try_lock().ok_or(SshError::Busy)?;
    if !host_valid(host) || port == 0 {
        return Err(SshError::InvalidConfiguration);
    }
    let mut command = Command::new(binary("ssh-keyscan")?);
    hidden(&mut command);
    command
        .args(["-T", "5", "-t", "ed25519", "-p"])
        .arg(port.to_string())
        .arg(host)
        .stdout(Stdio::piped());
    capture_host_key(command, host, port)
}

fn capture_host_key(mut command: Command, host: &str, port: u16) -> Result<HostKey, SshError> {
    let mut process = OwnedChild::spawn(&mut command)?;
    let mut output = process
        .child
        .stdout
        .take()
        .ok_or(SshError::ConnectionFailed)?;
    let reader = thread::Builder::new()
        .name("fono-ssh-keyscan".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            output
                .by_ref()
                .take(MAX_KEYSCAN_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        })
        .map_err(|_| SshError::Busy)?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let success = loop {
        match process.child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Err(_) => break false,
            Ok(None) if Instant::now() < deadline => thread::sleep(TICK),
            Ok(None) => break false,
        }
    };
    process.stop();
    let bytes = reader
        .join()
        .map_err(|_| SshError::ConnectionFailed)?
        .map_err(|_| SshError::ConnectionFailed)?;
    if !success || bytes.len() > MAX_KEYSCAN_BYTES {
        return Err(SshError::ConnectionFailed);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| SshError::ConnectionFailed)?;
    let bracketed = format!("[{host}]:{port}");
    let mut candidate = None;
    for line in text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        if fields.len() != 3
            || !(fields[0].eq_ignore_ascii_case(host) || fields[0].eq_ignore_ascii_case(&bracketed))
        {
            return Err(SshError::ConnectionFailed);
        }
        let key = parse_host_key(&format!("{} {}", fields[1], fields[2]))
            .map_err(|_| SshError::ConnectionFailed)?;
        if candidate.as_ref().is_some_and(|known| known != &key) {
            return Err(SshError::ConnectionFailed);
        }
        candidate = Some(key);
    }
    candidate.ok_or(SshError::ConnectionFailed)
}

struct PrivateHostFile {
    directory: PathBuf,
    path: PathBuf,
}

impl PrivateHostFile {
    fn new(host_key: &str) -> Result<Self, SshError> {
        let directory = std::env::temp_dir().join(format!("fono-ssh-{}", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        let mut builder = DirBuilder::new();
        #[cfg(not(unix))]
        let builder = DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .map_err(|_| SshError::ConnectionFailed)?;
        let file = Self {
            path: directory.join("known_hosts"),
            directory,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options
            .open(&file.path)
            .map_err(|_| SshError::ConnectionFailed)?;
        writeln!(output, "{HOST_ALIAS} {host_key}").map_err(|_| SshError::ConnectionFailed)?;
        Ok(file)
    }
}

impl Drop for PrivateHostFile {
    fn drop(&mut self) {
        // Never traverse a replaced directory or remove anything outside our
        // own unique, direct child of the current user's temporary directory.
        let Ok(parent) = std::env::temp_dir().canonicalize() else {
            return;
        };
        let Ok(target) = self.directory.canonicalize() else {
            return;
        };
        if target.parent() == Some(parent.as_path())
            && fs::symlink_metadata(&self.directory)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

#[derive(Clone)]
struct SshProcess {
    executable: PathBuf,
    settings: RemoteSshSettings,
    host_file: Arc<PrivateHostFile>,
}

impl SshProcess {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        hidden(&mut command);
        command
            .args(["-F", null_device(), "-T", "-W"])
            .arg(format!("127.0.0.1:{}", self.settings.server_port))
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "PasswordAuthentication=no",
                "-o",
                "KbdInteractiveAuthentication=no",
                "-o",
                "PreferredAuthentications=publickey",
                "-o",
                "NumberOfPasswordPrompts=0",
                "-o",
                "ForwardAgent=no",
                "-o",
                "ForwardX11=no",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "UpdateHostKeys=no",
                "-o",
                "VerifyHostKeyDNS=no",
                "-o",
                "HostKeyAlgorithms=ssh-ed25519",
                "-o",
                "ConnectTimeout=8",
                "-o",
                "ConnectionAttempts=1",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=2",
                "-o",
                "LogLevel=ERROR",
                "-o",
                "HostKeyAlias=fono-pinned-server",
            ])
            .arg("-o")
            .arg(format!("GlobalKnownHostsFile={}", null_device()))
            .arg("-o")
            // OpenSSH parses the option's value as its own whitespace-separated
            // list even though argv is already safely separated by Command.
            .arg(format!(
                "UserKnownHostsFile=\"{}\"",
                self.host_file.path.to_string_lossy().replace('\\', "/")
            ))
            .arg("-p")
            .arg(self.settings.port.to_string())
            .arg("-l")
            .arg(&self.settings.user);
        if let Some(identity) = &self.settings.identity_file {
            command
                .args(["-o", "IdentitiesOnly=yes", "-i"])
                .arg(identity);
        }
        command
            .arg(&self.settings.host)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        command
    }
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

struct Connection {
    cancel: Arc<AtomicBool>,
    socket: TcpStream,
    child: Arc<Mutex<Option<OwnedChild>>>,
    task: JoinHandle<()>,
}

impl Connection {
    fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.socket.shutdown(Shutdown::Both);
        if let Some(process) = self.child.lock().as_mut() {
            process.stop();
        }
    }

    fn join(self) {
        self.stop();
        let _ = self.task.join();
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn relay_input(
    mut socket: TcpStream,
    mut input: impl Write,
    cancel: &AtomicBool,
    last_io: &Mutex<Instant>,
) -> io::Result<()> {
    let mut bytes = [0u8; 16 * 1024];
    while !cancel.load(Ordering::Acquire) {
        match socket.read(&mut bytes) {
            Ok(0) => return Ok(()), // Preserve half-close: close SSH stdin only.
            Ok(count) => {
                *last_io.lock() = Instant::now();
                input.write_all(&bytes[..count])?;
            }
            Err(error) if retryable(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn relay_output(
    mut output: impl Read,
    mut socket: TcpStream,
    cancel: &AtomicBool,
    last_io: &Mutex<Instant>,
) -> io::Result<()> {
    let mut bytes = [0u8; 16 * 1024];
    while !cancel.load(Ordering::Acquire) {
        let count = match output.read(&mut bytes) {
            Ok(0) => return Ok(()),
            Ok(count) => count,
            Err(error) if retryable(&error) => continue,
            Err(error) => return Err(error),
        };
        *last_io.lock() = Instant::now();
        let mut remaining = &bytes[..count];
        while !remaining.is_empty() && !cancel.load(Ordering::Acquire) {
            match socket.write(remaining) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => {
                    *last_io.lock() = Instant::now();
                    remaining = &remaining[written..];
                }
                Err(error) if retryable(&error) => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

type ProcessFactory = Arc<dyn Fn() -> Command + Send + Sync>;

fn connection(socket: TcpStream, factory: ProcessFactory) -> io::Result<Connection> {
    socket.set_read_timeout(Some(IO_TICK))?;
    socket.set_write_timeout(Some(IO_TICK))?;
    socket.set_nodelay(true)?;
    let control = socket.try_clone()?;
    let input_socket = socket.try_clone()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let child = Arc::new(Mutex::new(None::<OwnedChild>));
    let task_cancel = cancel.clone();
    let task_child = child.clone();
    let task = thread::Builder::new()
        .name("fono-ssh-channel".into())
        .spawn(move || {
            let mut command = factory();
            let Ok(mut process) = OwnedChild::spawn(&mut command) else {
                let _ = socket.shutdown(Shutdown::Both);
                return;
            };
            let Some(input) = process.child.stdin.take() else {
                return;
            };
            let Some(output) = process.child.stdout.take() else {
                return;
            };
            *task_child.lock() = Some(process);
            let last_io = Arc::new(Mutex::new(Instant::now()));
            let input_cancel = task_cancel.clone();
            let input_child = task_child.clone();
            let input_activity = last_io.clone();
            let input = thread::Builder::new()
                .name("fono-ssh-input".into())
                .spawn(move || {
                    if relay_input(input_socket, input, &input_cancel, &input_activity).is_err() {
                        input_cancel.store(true, Ordering::Release);
                        if let Some(child) = input_child.lock().as_mut() {
                            child.stop();
                        }
                    }
                });
            let Ok(input) = input else {
                task_cancel.store(true, Ordering::Release);
                let _ = socket.shutdown(Shutdown::Both);
                if let Some(child) = task_child.lock().as_mut() {
                    child.stop();
                }
                task_child.lock().take();
                return;
            };
            let output_cancel = task_cancel.clone();
            let output_child = task_child.clone();
            let output_activity = last_io.clone();
            let output_socket = socket.try_clone();
            let output = thread::Builder::new()
                .name("fono-ssh-output".into())
                .spawn(move || {
                    if let Ok(socket) = output_socket {
                        let _ = relay_output(output, socket, &output_cancel, &output_activity);
                    }
                    output_cancel.store(true, Ordering::Release);
                    if let Some(child) = output_child.lock().as_mut() {
                        child.stop();
                    }
                });
            let Ok(output) = output else {
                task_cancel.store(true, Ordering::Release);
                let _ = socket.shutdown(Shutdown::Both);
                if let Some(child) = task_child.lock().as_mut() {
                    child.stop();
                }
                let _ = input.join();
                task_child.lock().take();
                return;
            };
            while !task_cancel.load(Ordering::Acquire) && last_io.lock().elapsed() < IDLE_LIMIT {
                // Do not stop when the process exits: its stdout pipe can
                // still contain the final response bytes. The output reader
                // drains them before it signals completion.
                thread::sleep(TICK);
            }
            task_cancel.store(true, Ordering::Release);
            let _ = socket.shutdown(Shutdown::Both);
            if let Some(child) = task_child.lock().as_mut() {
                child.stop();
            }
            let _ = input.join();
            let _ = output.join();
            task_child.lock().take();
        })?;
    Ok(Connection {
        cancel,
        socket: control,
        child,
        task,
    })
}

pub(crate) struct Tunnel {
    url: String,
    stop: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
}

impl Tunnel {
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    fn is_alive(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    fn start(factory: ProcessFactory) -> Result<Self, SshError> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| SshError::ConnectionFailed)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| SshError::ConnectionFailed)?;
        let address = listener
            .local_addr()
            .map_err(|_| SshError::ConnectionFailed)?;
        let stop = Arc::new(AtomicBool::new(false));
        let accept_stop = stop.clone();
        let task = thread::Builder::new()
            .name("fono-ssh-listener".into())
            .spawn(move || {
                let mut connections = Vec::<Connection>::new();
                while !accept_stop.load(Ordering::Acquire) {
                    let mut index = 0;
                    while index < connections.len() {
                        if connections[index].task.is_finished() {
                            connections.swap_remove(index).join();
                        } else {
                            index += 1;
                        }
                    }
                    match listener.accept() {
                        Ok((socket, _)) if connections.len() < MAX_CONNECTIONS => {
                            if let Ok(connection) = connection(socket, factory.clone()) {
                                connections.push(connection);
                            }
                        }
                        Ok((socket, _)) => {
                            let _ = socket.shutdown(Shutdown::Both);
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock
                                    | io::ErrorKind::Interrupted
                                    | io::ErrorKind::ConnectionAborted
                            ) =>
                        {
                            thread::sleep(TICK)
                        }
                        Err(_) => break,
                    }
                }
                // Signal every worker before joining any one of them.
                for connection in &connections {
                    connection.stop();
                }
                for connection in connections {
                    connection.join();
                }
            })
            .map_err(|_| SshError::Busy)?;
        Ok(Self {
            url: format!("http://{address}"),
            stop,
            task: Some(task),
        })
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}

pub(crate) fn preview(settings: &RemoteSshSettings) -> Result<Tunnel, SshError> {
    validate(settings)?;
    if settings
        .identity_file
        .as_ref()
        .is_some_and(|file| !file.is_file())
    {
        return Err(SshError::IdentityMissing);
    }
    let process = SshProcess {
        executable: binary("ssh")?,
        settings: settings.clone(),
        host_file: Arc::new(PrivateHostFile::new(
            &parse_host_key(&settings.host_key)?.host_key,
        )?),
    };
    Tunnel::start(Arc::new(move || process.command()))
}

struct SavedTunnel {
    settings: RemoteSshSettings,
    tunnel: Tunnel,
}
static SAVED: OnceLock<Mutex<Option<SavedTunnel>>> = OnceLock::new();

pub(crate) fn resolve_saved(settings: &RemoteSshSettings) -> Result<String, SshError> {
    let mut saved = SAVED.get_or_init(|| Mutex::new(None)).lock();
    if let Some(current) = saved
        .as_ref()
        .filter(|current| &current.settings == settings && current.tunnel.is_alive())
    {
        return Ok(current.tunnel.url().to_owned());
    }
    let tunnel = preview(settings)?;
    let url = tunnel.url().to_owned();
    *saved = Some(SavedTunnel {
        settings: settings.clone(),
        tunnel,
    });
    Ok(url)
}

pub(crate) fn shutdown() {
    if let Some(saved) = SAVED.get() {
        saved.lock().take();
    }
}

#[cfg(test)]
mod tests;
