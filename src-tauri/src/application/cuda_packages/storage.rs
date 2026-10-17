use super::{check_cancel, invalid, manifest, PackageError, Result, VerifiedPackage};
use bzip2::read::BzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|_| PackageError::Storage)
}

pub(super) fn reject_links(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(PackageError::Storage);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(PackageError::Storage);
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(PackageError::Storage);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(PackageError::Storage),
        }
    }
    Ok(())
}

fn directory(path: &Path) -> Result<()> {
    reject_links(path)?;
    let mut ancestors: Vec<_> = path.ancestors().collect();
    ancestors.reverse();
    let mut guards = Vec::new();
    for ancestor in ancestors {
        reject_links(ancestor)?;
        if !ancestor.exists() {
            io(fs::create_dir(ancestor))?;
        }
        guards.push(protected_open(ancestor, true)?);
    }
    Ok(())
}

fn protected_open(path: &Path, directory: bool) -> Result<File> {
    reject_links(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Signed files use FILE_SHARE_READ: verified bytes cannot be
        // overwritten, unlinked or replaced while a worker owns this guard.
        options.share_mode(1).custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
        if directory {
            // FILE_TRAVERSE | FILE_READ_ATTRIBUTES participates in Windows'
            // delete-share checks without requiring directory-list access on
            // profile/drive ancestors. An attribute-only handle does not
            // prevent even an empty directory from being renamed. Directory
            // guards share READ|WRITE so moving an owned child between guarded
            // parents is allowed; withholding DELETE still blocks replacement
            // of the guarded directory itself. Payload child creation remains
            // forbidden by its persistent sealed DACL.
            options
                .access_mode(0xa0)
                .share_mode(3)
                .custom_flags(0x02200000);
        }
    }
    let file = io(options.open(path))?;
    let metadata = io(file.metadata())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(PackageError::Storage);
        }
    }
    reject_links(path)?;
    if metadata.is_dir() != directory {
        return Err(PackageError::Storage);
    }
    Ok(file)
}

fn guard_directories(root: &Path, slot: &Path) -> Result<Vec<File>> {
    reject_links(root)?;
    let relative = slot.strip_prefix(root).map_err(|_| PackageError::Storage)?;
    let mut current = root.to_owned();
    // Protect every existing ancestor, including AppData/profile directories.
    // Guarding only the package root leaves a rename-to-junction race in its
    // parents between path validation and opening a child file.
    let mut ancestors: Vec<_> = root.ancestors().collect();
    ancestors.reverse();
    let mut guards = Vec::new();
    for ancestor in ancestors {
        guards.push(protected_open(ancestor, true)?);
    }
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(PackageError::Storage);
        }
        current.push(component);
        guards.push(protected_open(&current, true)?);
    }
    Ok(guards)
}

pub(super) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    reject_links(path)?;
    let file = io(File::open(path))?;
    if !io(file.metadata())?.is_file() || io(file.metadata())?.len() > limit {
        return Err(PackageError::Storage);
    }
    let mut bytes = Vec::new();
    io(file.take(limit + 1).read_to_end(&mut bytes))?;
    if bytes.len() as u64 > limit {
        return Err(PackageError::Storage);
    }
    Ok(bytes)
}

fn create(path: &Path) -> Result<File> {
    reject_links(path)?;
    io(OpenOptions::new().write(true).create_new(true).open(path))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = create(path)?;
    io(file.write_all(bytes))?;
    io(file.sync_all())
}

/// The lock is also cross-process on Windows, the only supported installation
/// target. It is deliberately retained through activation and cleanup.
pub(super) struct InstallLock {
    _file: File,
    _ancestors: Vec<File>,
}

pub(super) fn install_lock(root: &Path) -> Result<InstallLock> {
    directory(root)?;
    let path = root.join("install.lock");
    reject_links(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).custom_flags(0x00200000);
    }
    let guards = guard_directories(root, root)?;
    let file = io(options.open(path))?;
    let metadata = io(file.metadata())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(PackageError::Storage);
        }
    }
    if !metadata.is_file() {
        return Err(PackageError::Storage);
    }
    Ok(InstallLock {
        _file: file,
        _ancestors: guards,
    })
}

pub(super) struct Staging {
    pub path: PathBuf,
    // Retain both parents through publication/cleanup. A path check alone
    // does not prevent an ordinary owner from replacing staging/slots by a
    // junction immediately before a later create or rename operation.
    _parent_guards: Vec<File>,
    directory_guard: Option<File>,
    payload_guard: Option<File>,
    _promotion_guards: Vec<File>,
    cleanup_allowed: bool,
}

impl Staging {
    pub fn new(root: &Path) -> Result<Self> {
        let parent = root.join("staging");
        directory(&parent)?;
        let parent_guards = guard_directories(root, &parent)?;
        let path = parent.join(uuid::Uuid::new_v4().to_string());
        reject_links(&path)?;
        io(fs::create_dir(&path))?;
        let directory_guard = protected_open(&path, true)?;
        Ok(Self {
            path,
            _parent_guards: parent_guards,
            directory_guard: Some(directory_guard),
            payload_guard: None,
            _promotion_guards: Vec::new(),
            cleanup_allowed: true,
        })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.cleanup_allowed {
            // Once the leaf guards were released for promotion, the old UUID
            // namespace is no longer ours. Never delete an object subsequently
            // placed there, including when the atomic rename failed.
            return;
        }
        // Delete only this operation's random direct child. Never enumerate
        // or recursively remove another operation's or a published slot.
        if reject_links(&self.path).is_ok() {
            // Parent handles remain alive throughout the no-follow cleanup;
            // the leaf handles must close to permit directory removal.
            drop(self.payload_guard.take());
            drop(self.directory_guard.take());
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) fn extract(
    archive_path: &Path,
    stage: &mut Staging,
    manifest: &manifest::Manifest,
    bytes: &[u8],
    signature: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    let payload = stage.path.join("payload");
    directory(&payload)?;
    stage.payload_guard = Some(protected_open(&payload, true)?);
    let mut archive = tar::Archive::new(BzDecoder::new(protected_open(archive_path, false)?));
    let expected: BTreeMap<_, _> = manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut seen = BTreeSet::new();
    let entries = archive.entries().map_err(|_| invalid())?.raw(true);
    for entry in entries {
        check_cancel(cancel)?;
        let mut entry = entry.map_err(|_| invalid())?;
        let name = std::str::from_utf8(entry.path_bytes().as_ref())
            .map_err(|_| invalid())?
            .to_owned();
        let expected = expected.get(name.as_str()).ok_or_else(invalid)?;
        // No tar::unpack: flat exact filenames only, no PAX/GNU extensions,
        // directories, links, ADS, case aliases or extra bytes are accepted.
        if !entry.header().entry_type().is_file()
            || entry.header().link_name_bytes().is_some()
            || entry.size() != expected.size
            || !seen.insert(name.clone())
            || !manifest::valid_name(&name)
        {
            return Err(invalid());
        }
        let mut file = create(&payload.join(&name))?;
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            check_cancel(cancel)?;
            let read = entry.read(&mut buffer).map_err(|_| invalid())?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > expected.size {
                return Err(invalid());
            }
            digest.update(&buffer[..read]);
            io(file.write_all(&buffer[..read]))?;
        }
        if count != expected.size || format!("{:x}", digest.finalize()) != expected.sha256 {
            return Err(PackageError::Hash);
        }
        io(file.sync_all())?;
    }
    if seen.len() != expected.len() {
        return Err(invalid());
    }
    check_cancel(cancel)?;
    write_new(&stage.path.join("manifest.json"), bytes)?;
    write_new(&stage.path.join("manifest.json.sig"), signature.as_bytes())?;
    super::sealing::seal(&payload)?;
    exact_names(&payload, manifest::PAYLOAD_NAMES)?;
    check_cancel(cancel)?;
    Ok(())
}

pub(super) fn promote(root: &Path, stage: &mut Staging, hash: &str) -> Result<PathBuf> {
    let parent = root.join("slots");
    directory(&parent)?;
    stage._promotion_guards = guard_directories(root, &parent)?;
    let slot = parent.join(hash);
    reject_links(&slot)?;
    if !slot.exists() {
        // No more writes use stage.path after releasing these leaf handles.
        // Both source/destination parents remain protected. A substituted leaf
        // is moved as a leaf, never followed, and verify_slot still fails closed.
        stage.cleanup_allowed = false;
        drop(stage.payload_guard.take());
        drop(stage.directory_guard.take());
        // Same filesystem, atomic move, no replacement of a published slot.
        io(fs::rename(&stage.path, &slot))?;
    }
    Ok(slot)
}

fn exact_names(directory: &Path, expected: &[&str]) -> Result<()> {
    reject_links(directory)?;
    let mut actual = BTreeSet::new();
    for entry in io(fs::read_dir(directory))? {
        let entry = io(entry)?;
        reject_links(&entry.path())?;
        actual.insert(
            entry
                .file_name()
                .into_string()
                .map_err(|_| PackageError::Storage)?,
        );
    }
    if actual != expected.iter().map(|name| (*name).to_owned()).collect() {
        return Err(PackageError::Storage);
    }
    Ok(())
}

pub(super) fn validate_layout(package: &VerifiedPackage) -> Result<()> {
    let payload = package
        .worker_path()
        .parent()
        .ok_or(PackageError::Storage)?;
    let slot = payload.parent().ok_or(PackageError::Storage)?;
    super::sealing::verify(payload)?;
    exact_names(slot, &["manifest.json", "manifest.json.sig", "payload"])?;
    exact_names(payload, manifest::PAYLOAD_NAMES)
}

pub(super) fn verify_slot(
    root: &Path,
    slot: &Path,
    trust: &manifest::Trust,
    cancel: &AtomicBool,
) -> Result<VerifiedPackage> {
    let mut guards = guard_directories(root, &slot.join("payload"))?;
    super::sealing::verify(&slot.join("payload"))?;
    exact_names(slot, &["manifest.json", "manifest.json.sig", "payload"])?;
    exact_names(&slot.join("payload"), manifest::PAYLOAD_NAMES)?;
    let mut manifest_guard = protected_open(&slot.join("manifest.json"), false)?;
    let mut signature_guard = protected_open(&slot.join("manifest.json.sig"), false)?;
    let mut bytes = Vec::new();
    io((&mut manifest_guard)
        .take(manifest::MAX_MANIFEST + 1)
        .read_to_end(&mut bytes))?;
    let mut signature = String::new();
    io((&mut signature_guard)
        .take(manifest::MAX_SIGNATURE + 1)
        .read_to_string(&mut signature))?;
    let manifest = manifest::verify(&bytes, &signature, trust)?;
    let hash = manifest::hash(&bytes);
    if slot.file_name().and_then(|name| name.to_str()) != Some(&hash) {
        return Err(PackageError::Hash);
    }
    guards.push(manifest_guard);
    guards.push(signature_guard);
    for expected in &manifest.files {
        check_cancel(cancel)?;
        let mut file = protected_open(&slot.join("payload").join(&expected.path), false)?;
        if io(file.metadata())?.len() != expected.size {
            return Err(PackageError::Hash);
        }
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            check_cancel(cancel)?;
            let read = io(file.read(&mut buffer))?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > expected.size {
                return Err(PackageError::Hash);
            }
            digest.update(&buffer[..read]);
        }
        if count != expected.size || format!("{:x}", digest.finalize()) != expected.sha256 {
            return Err(PackageError::Hash);
        }
        guards.push(file);
    }
    Ok(VerifiedPackage {
        worker: slot.join("payload").join(manifest::WORKER),
        manifest,
        hash,
        _guards: Arc::new(guards),
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Accepted {
    revision: u64,
    hash: String,
}

#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    schema_version: u32,
    active: Option<Accepted>,
    high_water: BTreeMap<String, Accepted>,
}

pub(super) fn state(root: &Path) -> Result<State> {
    let path = root.join("state.json");
    reject_links(&path)?;
    if !path.exists() {
        return Ok(State {
            schema_version: 1,
            ..State::default()
        });
    }
    let state: State = serde_json::from_slice(&read_bounded(&path, manifest::MAX_MANIFEST)?)
        .map_err(|_| PackageError::Storage)?;
    if state.schema_version != 1 || state.high_water.len() > 128 {
        return Err(PackageError::Storage);
    }
    for value in state.high_water.values().chain(state.active.iter()) {
        if value.revision == 0
            || value.hash.len() != 64
            || !value
                .hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(PackageError::Storage);
        }
    }
    Ok(state)
}

pub(super) fn allow(state: &State, package: &manifest::Manifest, hash: &str) -> Result<()> {
    if let Some(previous) = state.high_water.get(&package.app_version) {
        if package.revision < previous.revision
            || (package.revision == previous.revision && hash != previous.hash)
        {
            return Err(PackageError::Rollback);
        }
    }
    Ok(())
}

pub(super) fn commit(root: &Path, package: &VerifiedPackage) -> Result<()> {
    let mut state = state(root)?;
    allow(&state, &package.manifest, &package.hash)?;
    let accepted = Accepted {
        revision: package.revision(),
        hash: package.hash.clone(),
    };
    state
        .high_water
        .insert(package.app_version().into(), accepted.clone());
    state.active = Some(accepted);
    if state.high_water.len() > 128 {
        return Err(PackageError::Storage);
    }
    let bytes = serde_json::to_vec(&state).map_err(|_| PackageError::Storage)?;
    let temporary = root.join(format!("state-{}.tmp", uuid::Uuid::new_v4()));
    write_new(&temporary, &bytes)?;
    let result = (|| {
        reject_links(&root.join("state.json"))?;
        atomic_replace(&temporary, &root.join("state.json"))?;
        #[cfg(not(windows))]
        io(File::open(root).and_then(|dir| dir.sync_all()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            },
        };
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|_| PackageError::Storage)
    }
    #[cfg(not(windows))]
    io(fs::rename(source, destination))
}

pub(super) fn active(root: &Path, trust: &manifest::Trust) -> Result<Option<VerifiedPackage>> {
    let state = state(root)?;
    let Some(active) = state.active.as_ref() else {
        return Ok(None);
    };
    let result = verify_slot(
        root,
        &root.join("slots").join(&active.hash),
        trust,
        &AtomicBool::new(false),
    );
    // A previously accepted package from another application version remains
    // on disk, but it must never be executed by this exact-version runtime.
    let package = match result {
        Err(PackageError::Compatibility) => return Ok(None),
        result => result?,
    };
    let high_water = state
        .high_water
        .get(package.app_version())
        .ok_or(PackageError::Rollback)?;
    if package.revision() != active.revision
        || high_water.revision != active.revision
        || high_water.hash != active.hash
    {
        return Err(PackageError::Rollback);
    }
    Ok(Some(package))
}
