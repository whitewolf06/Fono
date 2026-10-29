use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::{Report, Storage};
use crate::config::ServerConfig;

pub(super) fn inspect(config: &ServerConfig, report: &mut Report) {
    let model_read = readable_directory(&config.models.dir).is_ok();
    report.check("models_directory", true, model_read, directory_detail(model_read),
        "Create models.dir as a real directory and grant this service account read/traverse access.");
    let model_write = model_read && write_probe(&config.models.dir).is_ok();
    report.check("models_writable", false, model_write,
        if model_write { "Model directory accepts temporary writes" } else { "Model directory is not writable for model management" },
        "Use a writable model volume for model CLI operations; installed models can be served read-only.");
    let parent = config
        .server
        .tokens_file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let tokens_read = readable_directory(parent).is_ok();
    report.check("credentials_directory", true, tokens_read, directory_detail(tokens_read),
        "Grant this service account read/traverse access to the credential directory and keep the service mount read-only.");
    let temporary = std::env::temp_dir();
    let temporary_write = readable_directory(&temporary).is_ok() && write_probe(&temporary).is_ok();
    report.check(
        "temporary_directory",
        true,
        temporary_write,
        if temporary_write {
            "Private temporary write/read/remove probe passed"
        } else {
            "Temporary directory is missing, unsafe or not writable"
        },
        "Provide a real writable temporary directory, for example the Compose /tmp tmpfs.",
    );
    inspect_space(
        "models",
        &config.models.dir,
        64 * 1024 * 1024,
        false,
        report,
    );
    let upload_bytes = (config.server.max_upload_mb as u64).saturating_mul(1024 * 1024);
    inspect_space(
        "temporary",
        &temporary,
        upload_bytes.saturating_add(1024 * 1024),
        true,
        report,
    );
}

fn directory_detail(ok: bool) -> &'static str {
    if ok {
        "Directory is real, readable and traversable"
    } else {
        "Directory is missing, unsafe or unreadable"
    }
}

fn readable_directory(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    fs::read_dir(path)?.next().transpose()?;
    Ok(())
}

struct ProbeFile(PathBuf);
impl Drop for ProbeFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn write_probe(directory: &Path) -> io::Result<()> {
    let path = directory.join(format!(".fono-doctor-{}", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options.open(&path)?;
    let guard = ProbeFile(path);
    file.write_all(b"fono-doctor\n")?;
    file.sync_data()?;
    file.seek(SeekFrom::Start(0))?;
    let mut read = [0u8; 12];
    file.read_exact(&mut read)?;
    if read != *b"fono-doctor\n" {
        return Err(io::ErrorKind::InvalidData.into());
    }
    drop(file);
    fs::remove_file(&guard.0)?;
    Ok(())
}

fn inspect_space(
    area: &'static str,
    directory: &Path,
    minimum: u64,
    required: bool,
    report: &mut Report,
) {
    let result = disk_space(directory);
    let ok = result.as_ref().is_ok_and(|space| space.0 >= minimum);
    let (available, total) = result.map_or((None, None), |(a, t)| (Some(a), Some(t)));
    report.storage.push(Storage {
        area,
        available_bytes: available,
        total_bytes: total,
        recommended_bytes: minimum,
    });
    let name = if area == "models" {
        "models_space"
    } else {
        "temporary_space"
    };
    report.check(name, required, ok,
        match available {
            Some(bytes) => format!("{bytes} bytes available; {minimum} bytes recommended"),
            None => "Free space is unavailable".into(),
        }, if area == "models" { "Check the model filesystem or free space before model downloads." }
        else { "Provide enough temporary space for max_upload_mb plus probe overhead, or lower max_upload_mb." });
}

#[cfg(unix)]
fn disk_space(directory: &Path) -> io::Result<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(directory.as_os_str().as_bytes())
        .map_err(|_| io::ErrorKind::InvalidInput)?;
    let mut output = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL-terminated and output has the exact native layout.
    if unsafe { libc::statvfs(path.as_ptr(), output.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statvfs initialized output after returning success.
    let output = unsafe { output.assume_init() };
    let block = u128::from(output.f_frsize);
    let available = u128::from(output.f_bavail) * block;
    let total = u128::from(output.f_blocks) * block;
    if block == 0 || total == 0 || available > total {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok((
        available.min(u128::from(u64::MAX)) as u64,
        total.min(u128::from(u64::MAX)) as u64,
    ))
}

#[cfg(windows)]
fn disk_space(directory: &Path) -> io::Result<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let mut path: Vec<u16> = directory.as_os_str().encode_wide().collect();
    if path.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if !matches!(path.last(), Some(47 | 92)) {
        path.push(92);
    }
    path.push(0);
    let (mut available, mut total, mut free) = (0, 0, 0);
    // SAFETY: path is a NUL-terminated UTF-16 string; output integers are live.
    if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut available, &mut total, &mut free) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if total == 0 || available > total {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok((available, total))
}

#[cfg(not(any(unix, windows)))]
fn disk_space(_: &Path) -> io::Result<(u64, u64)> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_probe_preserves_existing_files_and_cleans_its_own_file() {
        let directory =
            std::env::temp_dir().join(format!("fono-doctor-fs-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("sentinel"), b"keep").unwrap();
        readable_directory(&directory).unwrap();
        write_probe(&directory).unwrap();
        assert_eq!(fs::read(directory.join("sentinel")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        let (available, total) = disk_space(&directory).unwrap();
        assert!(total > 0 && available <= total);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn unavailable_space_is_absent_and_insufficient_space_is_a_required_failure() {
        let mut report = Report::new();
        let missing = std::env::temp_dir().join(format!("fono-missing-{}", uuid::Uuid::new_v4()));
        inspect_space("models", &missing, 1, false, &mut report);
        assert_eq!(report.storage[0].available_bytes, None);
        assert_eq!(report.storage[0].total_bytes, None);
        assert!(!report.checks[0].ok && !report.checks[0].required);
        inspect_space(
            "temporary",
            &std::env::temp_dir(),
            u64::MAX,
            true,
            &mut report,
        );
        assert!(!report.checks[1].ok && report.checks[1].required);
        assert!(!report.required_checks_pass());
    }
    #[cfg(unix)]
    #[test]
    fn model_directory_probe_rejects_symbolic_ancestors() {
        let directory =
            std::env::temp_dir().join(format!("fono-doctor-link-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join("real")).unwrap();
        std::os::unix::fs::symlink(directory.join("real"), directory.join("link")).unwrap();
        assert!(readable_directory(&directory.join("link")).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
