use crate::{manifest::Release, Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) struct CacheLock {
    file: File,
    _directories: Vec<File>,
}

impl CacheLock {
    pub fn acquire(directory: &Path) -> Result<Self> {
        let directories = protect_directories(directory)?;
        reject_link(&directory.join("download.lock"))?;
        let file = open_regular(&directory.join("download.lock"), false)?;
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => Error::Busy,
            std::fs::TryLockError::Error(error) => Error::Cache(error),
        })?;
        Ok(Self {
            file,
            _directories: directories,
        })
    }
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct PartialState {
    pub validator: Option<String>,
    pub total: Option<u64>,
}

pub(crate) struct Cache {
    pub partial: PathBuf,
    pub installer: PathBuf,
    state: PathBuf,
    pub directory_handle: File,
}

impl Cache {
    pub fn for_release(directory: &Path, release: &Release) -> Result<Self> {
        let mut digest = Sha256::new();
        for value in [
            &release.version,
            release.url.as_str(),
            &release.signature_text,
        ] {
            digest.update(value.as_bytes());
            digest.update([0]);
        }
        let key = format!("{:x}", digest.finalize());
        let directory = directory.join(key);
        if !directory.exists() {
            fs::create_dir(&directory)?;
        }
        let directory_handle = protect_directory(&directory)?;
        Ok(Self {
            partial: directory.join("installer.part"),
            installer: directory.join("FonoInstaller.exe"),
            state: directory.join("resume.json"),
            directory_handle,
        })
    }

    pub fn load_state(&self) -> PartialState {
        if reject_link(&self.state).is_err() {
            return PartialState::default();
        }
        let Ok(file) = protected_read(&self.state) else {
            return PartialState::default();
        };
        let mut bytes = Vec::new();
        if file.take(4097).read_to_end(&mut bytes).is_err() || bytes.len() > 4096 {
            return PartialState::default();
        }
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub fn save_state(&self, state: &PartialState) -> Result<()> {
        let temporary = self.state.with_extension("tmp");
        reject_link(&self.state)?;
        reject_link(&temporary)?;
        let bytes = serde_json::to_vec(state).map_err(|_| Error::Manifest)?;
        let mut file = open_regular(&temporary, false)?;
        file.set_len(0)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if self.state.exists() {
            fs::remove_file(&self.state)?;
        }
        fs::rename(&temporary, &self.state)?;
        Ok(())
    }

    pub fn reset_partial(&self) -> Result<()> {
        reject_link(&self.partial)?;
        let file = open_regular(&self.partial, false)?;
        file.set_len(0)?;
        file.sync_all()?;
        self.save_state(&PartialState::default())
    }

    pub fn promote(&self) -> Result<()> {
        reject_link(&self.installer)?;
        reject_link(&self.partial)?;
        if self.installer.exists() {
            fs::remove_file(&self.installer)?;
        }
        fs::rename(&self.partial, &self.installer)?;
        if self.state.exists() {
            fs::remove_file(&self.state)?;
        }
        Ok(())
    }
}

pub(crate) fn protected_read(path: &Path) -> Result<File> {
    reject_link(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Keep the verified executable immutable until the caller releases
        // its VerifiedInstaller. Windows denies writes, replacement and delete.
        options.share_mode(1).custom_flags(0x0020_0000); // READ, OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    validate_handle(&file, false)?;
    Ok(file)
}

pub(crate) fn open_regular(path: &Path, append: bool) -> Result<File> {
    reject_link(path)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .append(append)
        .truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Preserve the file object used by the OS lock and each write. In
        // particular download.lock cannot be unlinked to create another lock.
        options.share_mode(3).custom_flags(0x0020_0000); // READ | WRITE, no DELETE
    }
    let file = options.open(path)?;
    validate_handle(&file, false)?;
    Ok(file)
}

fn validate_handle(file: &File, directory: bool) -> Result<()> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::Configuration);
        }
        if !directory {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{
                Foundation::HANDLE,
                Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
            };
            let mut information = BY_HANDLE_FILE_INFORMATION::default();
            // The handle is owned by File and remains valid for this call.
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
                .map_err(|_| Error::Configuration)?;
            if information.nNumberOfLinks != 1 {
                return Err(Error::Configuration);
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !directory && metadata.nlink() != 1 {
            return Err(Error::Configuration);
        }
    }
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(Error::Configuration);
    }
    Ok(())
}

pub(crate) fn reject_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
            };
            #[cfg(not(windows))]
            let reparse = metadata.file_type().is_symlink();
            if reparse {
                return Err(Error::Configuration);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Cache(error)),
    }
    Ok(())
}

fn protect_directories(path: &Path) -> Result<Vec<File>> {
    let mut ancestors: Vec<_> = path.ancestors().collect();
    ancestors.reverse();
    let mut handles = Vec::new();
    for directory in ancestors {
        reject_link(directory)?;
        if !directory.exists() {
            fs::create_dir(directory)?;
        }
        handles.push(protect_directory(directory)?);
    }
    Ok(handles)
}

fn protect_directory(path: &Path) -> Result<File> {
    reject_link(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0x80)
            .share_mode(1)
            .custom_flags(0x0220_0000);
    }
    let file = options.open(path)?;
    reject_link(path)?;
    validate_handle(&file, true)?;
    Ok(file)
}
