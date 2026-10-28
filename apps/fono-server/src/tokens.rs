//! Server bearer tokens. Only salted digests are persisted; a new secret is
//! returned once, after its record has been successfully published.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use fono_service::rest_api::{TokenScope, TokenVerifier};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

const MAX_TOKEN_FILE_BYTES: u64 = 256 * 1024;
const MAX_TOKENS: usize = 1024;
const TOKEN_PREFIX: &str = "fono_";
const HASH_DOMAIN: &[u8] = b"fono-server-token-sha256-v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    InvalidName,
    DuplicateName,
    NotFound,
    MissingFile,
    UnsafeFile,
    InvalidFile,
    Full,
    Io,
}

impl fmt::Display for TokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidName => "client name must use 1-64 ASCII letters, digits, _ or -",
            Self::DuplicateName => "client name already exists",
            Self::NotFound => "client name not found",
            Self::MissingFile => "token file does not exist",
            Self::UnsafeFile => "token file or its path is unsafe",
            Self::InvalidFile => "token file contains an invalid record",
            Self::Full => "token file has reached its size or client limit",
            Self::Io => "token file operation failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for TokenError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSummary {
    pub name: String,
    pub scope: TokenScope,
    pub created_at: String,
}

struct TokenRecord {
    name: String,
    scope: TokenScope,
    salt: String,
    digest: [u8; 32],
    created_at: String,
}

/// The verifier deliberately reads the file for each request. An atomic
/// rename takes effect on the next authorization, including revocations.
/// A missing, unreadable, or malformed file denies every token.
pub struct FileTokenVerifier {
    path: PathBuf,
}

impl FileTokenVerifier {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, TokenError> {
        let path = path.into();
        read_records(&path, false)?;
        Ok(Self { path })
    }
}

impl TokenVerifier for FileTokenVerifier {
    fn verify(&self, token: &str) -> bool {
        self.scope(token).is_some()
    }

    fn scope(&self, token: &str) -> Option<TokenScope> {
        if !valid_token_format(token) {
            return None;
        }
        let records = read_records(&self.path, false).ok()?;
        let mut recognize = 0u8;
        let mut admin = 0u8;
        for record in records {
            let digest = digest_token(&record.salt, token);
            let equal = digest
                .as_slice()
                .ct_eq(record.digest.as_slice())
                .unwrap_u8();
            recognize |= equal & u8::from(record.scope == TokenScope::Recognize);
            admin |= equal & u8::from(record.scope == TokenScope::Admin);
        }
        if admin != 0 {
            Some(TokenScope::Admin)
        } else if recognize != 0 {
            Some(TokenScope::Recognize)
        } else {
            None
        }
    }
}

pub fn create(path: &Path, name: &str, scope: TokenScope) -> Result<String, TokenError> {
    if !valid_name(name) {
        return Err(TokenError::InvalidName);
    }
    let _lock = lock_file(path)?;
    let mut records = read_records(path, true)?;
    if records.iter().any(|record| record.name == name) {
        return Err(TokenError::DuplicateName);
    }
    if records.len() >= MAX_TOKENS {
        return Err(TokenError::Full);
    }
    let secret = format!(
        "{TOKEN_PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let salt = Uuid::new_v4().simple().to_string();
    let digest = digest_token(&salt, &secret);
    records.push(TokenRecord {
        name: name.to_owned(),
        scope,
        salt,
        digest,
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    });
    write_records(path, &records)?;
    Ok(secret)
}

pub fn list(path: &Path) -> Result<Vec<TokenSummary>, TokenError> {
    let mut summaries: Vec<_> = read_records(path, true)?
        .into_iter()
        .map(|record| TokenSummary {
            name: record.name,
            scope: record.scope,
            created_at: record.created_at,
        })
        .collect();
    summaries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(summaries)
}

pub fn revoke(path: &Path, name: &str) -> Result<(), TokenError> {
    if !valid_name(name) {
        return Err(TokenError::InvalidName);
    }
    let _lock = lock_file(path)?;
    let mut records = read_records(path, false)?;
    let previous_len = records.len();
    records.retain(|record| record.name != name);
    if records.len() == previous_len {
        return Err(TokenError::NotFound);
    }
    write_records(path, &records)
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_token_format(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == TOKEN_PREFIX.len() + 64
        && bytes.starts_with(TOKEN_PREFIX.as_bytes())
        && bytes[TOKEN_PREFIX.len()..]
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn digest_token(salt: &str, token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(HASH_DOMAIN);
    hasher.update(salt.as_bytes());
    hasher.update(token.as_bytes());
    hasher.finalize().into()
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode_digest(value: &str) -> Result<[u8; 32], TokenError> {
    if value.len() != 64 {
        return Err(TokenError::InvalidFile);
    }
    let mut decoded = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0]).ok_or(TokenError::InvalidFile)?;
        let low = hex_nibble(pair[1]).ok_or(TokenError::InvalidFile)?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn check_parents(path: &Path) -> Result<&Path, TokenError> {
    if path.file_name().is_none()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(TokenError::UnsafeFile);
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for ancestor in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| TokenError::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(TokenError::UnsafeFile);
        }
    }
    Ok(parent)
}

#[cfg(unix)]
fn check_unix_file(metadata: &fs::Metadata) -> Result<(), TokenError> {
    if metadata.mode() & 0o777 != 0o600 || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(TokenError::UnsafeFile);
    }
    Ok(())
}

fn open_records_file(path: &Path, allow_missing: bool) -> Result<Option<File>, TokenError> {
    check_parents(path)?;
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {
            return Ok(None);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(TokenError::MissingFile);
        }
        Err(_) => return Err(TokenError::Io),
    };
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(TokenError::UnsafeFile);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path).map_err(|_| TokenError::Io)?;
    let after = file.metadata().map_err(|_| TokenError::Io)?;
    if !after.is_file() {
        return Err(TokenError::UnsafeFile);
    }
    #[cfg(unix)]
    {
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err(TokenError::UnsafeFile);
        }
        check_unix_file(&after)?;
    }
    Ok(Some(file))
}

fn read_records(path: &Path, allow_missing: bool) -> Result<Vec<TokenRecord>, TokenError> {
    let Some(file) = open_records_file(path, allow_missing)? else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    file.take(MAX_TOKEN_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| TokenError::Io)?;
    if bytes.len() as u64 > MAX_TOKEN_FILE_BYTES {
        return Err(TokenError::Full);
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| TokenError::InvalidFile)?;
    let mut records = Vec::new();
    let mut names = HashSet::new();
    for line in source.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 5 || !valid_name(fields[0]) || !names.insert(fields[0]) {
            return Err(TokenError::InvalidFile);
        }
        let scope = match fields[1] {
            "recognize" => TokenScope::Recognize,
            "admin" => TokenScope::Admin,
            _ => return Err(TokenError::InvalidFile),
        };
        let salt = fields[2];
        if salt.len() != 32 || !salt.bytes().all(|byte| hex_nibble(byte).is_some()) {
            return Err(TokenError::InvalidFile);
        }
        let digest = hex_decode_digest(fields[3])?;
        let created_at = fields[4];
        if !created_at.ends_with('Z') || chrono::DateTime::parse_from_rfc3339(created_at).is_err() {
            return Err(TokenError::InvalidFile);
        }
        records.push(TokenRecord {
            name: fields[0].to_owned(),
            scope,
            salt: salt.to_owned(),
            digest,
            created_at: created_at.to_owned(),
        });
        if records.len() > MAX_TOKENS {
            return Err(TokenError::Full);
        }
    }
    Ok(records)
}

fn lock_file(path: &Path) -> Result<File, TokenError> {
    let parent = check_parents(path)?;
    let mut name = OsString::from(path.file_name().ok_or(TokenError::UnsafeFile)?);
    name.push(".lock");
    let lock_path = parent.join(name);
    let existing = match fs::symlink_metadata(&lock_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(TokenError::Io),
    };
    if existing
        .as_ref()
        .is_some_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(TokenError::UnsafeFile);
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let file = options.open(lock_path).map_err(|_| TokenError::Io)?;
    #[cfg(unix)]
    {
        if existing.is_none() {
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| TokenError::Io)?;
        }
        check_unix_file(&file.metadata().map_err(|_| TokenError::Io)?)?;
    }
    file.lock().map_err(|_| TokenError::Io)?;
    Ok(file)
}

#[cfg(windows)]
fn publish_temp(temp: &Path, target: &Path) -> Result<(), TokenError> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        #[link_name = "ReplaceFileW"]
        fn replace_file_w(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }

    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(TokenError::UnsafeFile)
        }
        Ok(_) => {
            // A backup is essential: ReplaceFileW can move the old file out
            // before reporting failure (ERROR_UNABLE_TO_MOVE_REPLACEMENT_2).
            // If recovery fails, both backup and temp remain for inspection;
            // authorization fails closed while the main path is absent.
            let parent = target.parent().ok_or(TokenError::UnsafeFile)?;
            let mut backup_name = OsString::from(".");
            backup_name.push(target.file_name().ok_or(TokenError::UnsafeFile)?);
            backup_name.push(format!(".backup.{}", Uuid::new_v4().simple()));
            let backup = parent.join(backup_name);
            match fs::symlink_metadata(&backup) {
                Ok(_) => return Err(TokenError::UnsafeFile),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(TokenError::Io),
            }
            let replaced: Vec<u16> = target
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let replacement: Vec<u16> = temp
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let backup_wide: Vec<u16> = backup
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let status = unsafe {
                replace_file_w(
                    replaced.as_ptr(),
                    replacement.as_ptr(),
                    backup_wide.as_ptr(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status == 0 {
                let code = std::io::Error::last_os_error().raw_os_error();
                let target_missing = matches!(
                    fs::symlink_metadata(target),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound
                );
                if code == Some(1177)
                    && target_missing
                    && fs::symlink_metadata(&backup).is_ok_and(|metadata| {
                        metadata.is_file() && !metadata.file_type().is_symlink()
                    })
                {
                    let _ = fs::rename(&backup, target);
                }
                Err(TokenError::Io)
            } else {
                // The old file contains only salted digests. An unsuccessful
                // cleanup leaves a private backup rather than changing the
                // outcome of a completed create/revoke.
                if fs::symlink_metadata(&backup)
                    .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                {
                    let _ = fs::remove_file(&backup);
                }
                Ok(())
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(temp, target).map_err(|_| TokenError::Io)
        }
        Err(_) => Err(TokenError::Io),
    }
}

#[cfg(not(windows))]
fn publish_temp(temp: &Path, target: &Path) -> Result<(), TokenError> {
    fs::rename(temp, target).map_err(|_| TokenError::Io)
}

fn write_records(path: &Path, records: &[TokenRecord]) -> Result<(), TokenError> {
    let parent = check_parents(path)?;
    let mut contents = String::new();
    for record in records {
        let scope = match record.scope {
            TokenScope::Recognize => "recognize",
            TokenScope::Admin => "admin",
        };
        contents.push_str(&record.name);
        contents.push('\t');
        contents.push_str(scope);
        contents.push('\t');
        contents.push_str(&record.salt);
        contents.push('\t');
        contents.push_str(&hex_encode(&record.digest));
        contents.push('\t');
        contents.push_str(&record.created_at);
        contents.push('\n');
    }
    if contents.len() as u64 > MAX_TOKEN_FILE_BYTES {
        return Err(TokenError::Full);
    }
    let mut temp_name = OsString::from(".");
    temp_name.push(path.file_name().ok_or(TokenError::UnsafeFile)?);
    temp_name.push(format!(".tmp.{}", Uuid::new_v4().simple()));
    let temp_path = parent.join(temp_name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let mut temp = options.open(&temp_path).map_err(|_| TokenError::Io)?;
    #[cfg(windows)]
    let mut entered_publish = false;
    let result = (|| {
        #[cfg(unix)]
        temp.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| TokenError::Io)?;
        temp.write_all(contents.as_bytes())
            .map_err(|_| TokenError::Io)?;
        temp.sync_all().map_err(|_| TokenError::Io)?;
        drop(temp);
        #[cfg(windows)]
        {
            entered_publish = true;
        }
        publish_temp(&temp_path, path)?;
        #[cfg(unix)]
        {
            // Once published, an error cannot be rolled back without risking
            // an orphaned secret that was never shown to the caller.
            let _ = File::open(parent).and_then(|directory| directory.sync_all());
        }
        Ok(())
    })();
    #[cfg(windows)]
    if result.is_err() && !entered_publish {
        let _ = fs::remove_file(&temp_path);
    }
    #[cfg(not(windows))]
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        directory: PathBuf,
        tokens: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "fono-server-tokens-{}-{}",
                std::process::id(),
                Uuid::new_v4().simple()
            ));
            fs::create_dir(&directory).unwrap();
            let tokens = directory.join("tokens");
            Self { directory, tokens }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if self
                .directory
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("fono-server-tokens-"))
            {
                let _ = fs::remove_dir_all(&self.directory);
            }
        }
    }

    #[test]
    fn create_lists_only_metadata_and_revoke_takes_effect_without_restart() {
        let fixture = Fixture::new();
        assert!(list(&fixture.tokens).unwrap().is_empty());
        assert!(matches!(
            FileTokenVerifier::open(&fixture.tokens),
            Err(TokenError::MissingFile)
        ));
        let recognize = create(&fixture.tokens, "desktop-1", TokenScope::Recognize).unwrap();
        let admin = create(&fixture.tokens, "operator", TokenScope::Admin).unwrap();
        assert!(valid_token_format(&recognize));
        assert_ne!(recognize, admin);
        let stored = fs::read_to_string(&fixture.tokens).unwrap();
        assert!(!stored.contains(&recognize));
        assert!(!stored.contains(&admin));
        assert_eq!(stored.lines().count(), 2);
        let listed = list(&fixture.tokens).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "desktop-1");
        assert_eq!(listed[0].scope, TokenScope::Recognize);
        assert_eq!(listed[1].name, "operator");
        assert_eq!(listed[1].scope, TokenScope::Admin);
        assert!(listed.iter().all(|entry| entry.created_at.ends_with('Z')));

        let verifier = FileTokenVerifier::open(&fixture.tokens).unwrap();
        assert_eq!(verifier.scope(&recognize), Some(TokenScope::Recognize));
        assert_eq!(verifier.scope(&admin), Some(TokenScope::Admin));
        assert!(verifier.verify(&recognize));
        assert_eq!(verifier.scope("fono_bad"), None);
        revoke(&fixture.tokens, "desktop-1").unwrap();
        assert_eq!(verifier.scope(&recognize), None);
        assert_eq!(verifier.scope(&admin), Some(TokenScope::Admin));
        assert_eq!(list(&fixture.tokens).unwrap().len(), 1);
    }

    #[test]
    fn malformed_file_fails_closed_and_invalid_or_duplicate_names_do_not_write() {
        let fixture = Fixture::new();
        assert_eq!(
            create(&fixture.tokens, "bad name", TokenScope::Admin),
            Err(TokenError::InvalidName)
        );
        assert!(!fixture.tokens.exists());
        let secret = create(&fixture.tokens, "client", TokenScope::Recognize).unwrap();
        assert_eq!(
            create(&fixture.tokens, "client", TokenScope::Admin),
            Err(TokenError::DuplicateName)
        );
        let verifier = FileTokenVerifier::open(&fixture.tokens).unwrap();
        fs::write(&fixture.tokens, b"malformed\n").unwrap();
        assert_eq!(verifier.scope(&secret), None);
        assert!(matches!(
            list(&fixture.tokens),
            Err(TokenError::InvalidFile)
        ));
        assert!(matches!(
            FileTokenVerifier::open(&fixture.tokens),
            Err(TokenError::InvalidFile)
        ));
        fs::remove_file(&fixture.tokens).unwrap();
        assert_eq!(verifier.scope(&secret), None);
        assert!(matches!(
            revoke(&fixture.tokens, "client"),
            Err(TokenError::MissingFile)
        ));
    }

    #[test]
    fn token_file_records_have_fixed_salt_digest_and_timestamp_fields() {
        let fixture = Fixture::new();
        create(&fixture.tokens, "client", TokenScope::Admin).unwrap();
        let stored = fs::read_to_string(&fixture.tokens).unwrap();
        let fields: Vec<_> = stored.trim_end().split('\t').collect();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0], "client");
        assert_eq!(fields[1], "admin");
        assert_eq!(fields[2].len(), 32);
        assert_eq!(fields[3].len(), 64);
        assert!(chrono::DateTime::parse_from_rfc3339(fields[4]).is_ok());
        #[cfg(unix)]
        {
            let metadata = fs::metadata(&fixture.tokens).unwrap();
            assert_eq!(metadata.mode() & 0o777, 0o600);
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        }
    }

    #[cfg(windows)]
    #[test]
    fn failed_replacement_preserves_the_existing_token_file() {
        let fixture = Fixture::new();
        let old = create(&fixture.tokens, "original", TokenScope::Admin).unwrap();
        let mut permissions = fs::metadata(&fixture.tokens).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&fixture.tokens, permissions).unwrap();

        let outcome = create(&fixture.tokens, "second", TokenScope::Recognize);
        let mut permissions = fs::metadata(&fixture.tokens).unwrap().permissions();
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "Windows-only test restores the original file attribute"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(&fixture.tokens, permissions).unwrap();
        let verifier = FileTokenVerifier::open(&fixture.tokens).unwrap();
        assert_eq!(verifier.scope(&old), Some(TokenScope::Admin));
        if let Ok(second) = outcome {
            assert_eq!(verifier.scope(&second), Some(TokenScope::Recognize));
        } else {
            assert_eq!(list(&fixture.tokens).unwrap().len(), 1);
        }

        let temp = fixture.directory.join("replacement-temp");
        fs::write(&temp, b"private test fixture").unwrap();
        let occupied = fixture.directory.join("occupied");
        fs::create_dir(&occupied).unwrap();
        assert_eq!(publish_temp(&temp, &occupied), Err(TokenError::UnsafeFile));
        assert!(temp.exists());
        assert!(occupied.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_loose_permissions_and_symlinked_file_or_parent() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        create(&fixture.tokens, "client", TokenScope::Recognize).unwrap();
        fs::set_permissions(&fixture.tokens, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            FileTokenVerifier::open(&fixture.tokens),
            Err(TokenError::UnsafeFile)
        ));
        fs::set_permissions(&fixture.tokens, fs::Permissions::from_mode(0o600)).unwrap();
        let linked_file = fixture.directory.join("linked-tokens");
        symlink(&fixture.tokens, &linked_file).unwrap();
        assert!(matches!(
            FileTokenVerifier::open(&linked_file),
            Err(TokenError::UnsafeFile)
        ));
        let linked_parent = fixture.directory.join("linked-parent");
        symlink(&fixture.directory, &linked_parent).unwrap();
        assert!(matches!(
            FileTokenVerifier::open(linked_parent.join("tokens")),
            Err(TokenError::UnsafeFile)
        ));
    }
}
