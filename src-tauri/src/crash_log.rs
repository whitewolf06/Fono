//! Synchronous, bounded crash evidence for both abort and unwind panic profiles.
//! Never format the panic payload: it may contain dictated text or credentials.
use std::{
    backtrace::Backtrace,
    fmt::{self, Write as _},
    fs::{File, OpenOptions},
    io::{self, Seek as _, Write as _},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 256 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;
static WRITING: AtomicBool = AtomicBool::new(false);

/// Install before tracing and application state. Resolving the directory stays
/// inside the hook so even a panic during logging initialization is recorded.
pub(crate) fn install(resolve_directory: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) {
    // Replacing the default hook also prevents its stderr output from exposing
    // an arbitrary panic payload. A hook runs before panic=abort terminates.
    std::panic::set_hook(Box::new(move |info| {
        if WRITING.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(directory) = resolve_directory() {
            let _ = write_record(&directory, info.location());
        }
        WRITING.store(false, Ordering::Release);
    }));
}

fn write_record(
    directory: &std::path::Path,
    location: Option<&std::panic::Location<'_>>,
) -> io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(directory.join("crash.log"))?;
    // Reserve one complete record. Old technical evidence is discarded only
    // when the bounded file cannot hold another maximal record.
    if file.metadata()?.len() > MAX_LOG_BYTES - MAX_RECORD_BYTES as u64 {
        file.set_len(0)?;
    }
    file.seek(io::SeekFrom::End(0))?;
    let mut writer = BoundedWriter {
        file: &mut file,
        remaining: MAX_RECORD_BYTES,
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let _ = writeln!(
        writer,
        "\nFono fatal panic; payload omitted\nunix_seconds={timestamp}"
    );
    if let Some(location) = location {
        let _ = writeln!(
            writer,
            "location={}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    }
    // Persist the location first, before potentially expensive symbolization.
    writer.file.flush()?;
    writer.file.sync_all()?;
    let backtrace = Backtrace::force_capture();
    let _ = writeln!(writer, "backtrace:\n{backtrace}");
    writer.file.flush()?;
    writer.file.sync_all()
}

struct BoundedWriter<'a> {
    file: &'a mut File,
    remaining: usize,
}

impl fmt::Write for BoundedWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let mut end = value.len().min(self.remaining);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.file
            .write_all(&value.as_bytes()[..end])
            .map_err(|_| fmt::Error)?;
        self.remaining -= end;
        if end < value.len() {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn abort_profile_writes_bounded_location_and_backtrace_without_panic_payload() {
        let fixture = std::env::temp_dir().join(format!(
            "fono-crash-hook-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&fixture).unwrap();
        let source = fixture.join("crash-log-abort-fixture.rs");
        let executable = fixture.join(if cfg!(windows) {
            "fixture.exe"
        } else {
            "fixture"
        });
        let module = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/crash_log.rs");
        std::fs::write(&source, format!(
            "#[path = {module:?}] mod crash_log;\nfn main() {{\nlet directory = std::path::PathBuf::from(std::env::args_os().nth(1).unwrap());\ncrash_log::install(move || Some(directory.clone()));\npanic!(\"PRIVATE_TRANSCRIPT_AND_API_KEY_SENTINEL\");\n}}\n"
        )).unwrap();
        let compiled = Command::new("rustc")
            .arg("--edition=2021")
            .arg("-C")
            .arg("panic=abort")
            .arg("-C")
            .arg("opt-level=2")
            .arg("-C")
            .arg("debuginfo=1")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let log = fixture.join("crash.log");
        std::fs::write(&log, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        let crashed = Command::new(&executable).arg(&fixture).output().unwrap();
        assert!(!crashed.status.success());
        let evidence = std::fs::read_to_string(&log).unwrap();
        assert!(evidence.contains("location="));
        assert!(evidence.contains("crash-log-abort-fixture.rs:5:"));
        assert!(evidence.contains("backtrace:\n"));
        assert!(evidence.len() <= MAX_RECORD_BYTES);
        let stdout = String::from_utf8_lossy(&crashed.stdout);
        let stderr = String::from_utf8_lossy(&crashed.stderr);
        for output in [evidence.as_str(), stdout.as_ref(), stderr.as_ref()] {
            assert!(!output.contains("PRIVATE_TRANSCRIPT_AND_API_KEY_SENTINEL"));
        }
        for file in [source, executable, log] {
            let _ = std::fs::remove_file(file);
        }
        let _ = std::fs::remove_dir(fixture);
    }
}
