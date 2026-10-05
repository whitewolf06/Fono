use crate::{
    cache::{Cache, CacheLock, PartialState},
    manifest::Release,
    signature::InstallerSignature,
    *,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    version: String,
    message: String,
    public_key: String,
    signature: String,
}

fn fixture(legacy: bool) -> Fixture {
    serde_json::from_str(if legacy {
        include_str!("../tests/fixtures/legacy-signature.json")
    } else {
        include_str!("../tests/fixtures/prehashed-signature.json")
    })
    .unwrap()
}

pub(crate) struct TestDirectory(pub PathBuf);
impl TestDirectory {
    pub fn new() -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fono-setup-test-{}-{timestamp}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn test_release(url: &str) -> Release {
    let fixture = fixture(false);
    Release {
        version: fixture.version.clone(),
        url: reqwest::Url::parse(url).unwrap(),
        signature: InstallerSignature::parse(
            &fixture.signature,
            &fixture.public_key,
            &fixture.version,
        )
        .unwrap(),
        signature_text: fixture.signature,
    }
}

pub(crate) fn test_config() -> Config {
    Config {
        endpoint: "https://github.com/whitewolf06/fono/releases/latest/download/latest.json".into(),
        public_key: fixture(false).public_key,
        repository: "whitewolf06/fono".into(),
    }
}

#[test]
fn verifies_ed_and_ed_prehashed_then_rejects_tamper() {
    for legacy in [false, true] {
        let fixture = fixture(legacy);
        let signature_text =
            String::from_utf8(STANDARD.decode(&fixture.signature).unwrap()).unwrap();
        let record = STANDARD
            .decode(signature_text.lines().nth(1).unwrap())
            .unwrap();
        assert_eq!(&record[..2], if legacy { b"Ed" } else { b"ED" });
        let directory = TestDirectory::new();
        let path = directory.0.join("fixture.exe");
        fs::write(&path, STANDARD.decode(fixture.message).unwrap()).unwrap();
        verify_installer(
            &path,
            &fixture.signature,
            &fixture.public_key,
            &fixture.version,
        )
        .unwrap();
        assert!(matches!(
            verify_installer(&path, &fixture.signature, &fixture.public_key, "0.6.11"),
            Err(Error::Version)
        ));
        fs::write(&path, b"tampered executable").unwrap();
        assert!(matches!(
            verify_installer(
                &path,
                &fixture.signature,
                &fixture.public_key,
                &fixture.version
            ),
            Err(Error::Signature)
        ));
    }
}

#[test]
fn rejects_changed_trusted_comment_and_extra_signature_lines() {
    let fixture = fixture(false);
    let directory = TestDirectory::new();
    let path = directory.0.join("fixture.exe");
    fs::write(&path, STANDARD.decode(&fixture.message).unwrap()).unwrap();
    let text = String::from_utf8(STANDARD.decode(&fixture.signature).unwrap()).unwrap();
    let changed = STANDARD.encode(text.replace("version:0.6.12", "version:0.6.13"));
    assert!(matches!(
        verify_installer(&path, &changed, &fixture.public_key, "0.6.13"),
        Err(Error::Signature)
    ));
    let changed = STANDARD.encode(format!("{text}unexpected line\n"));
    assert!(matches!(
        verify_installer(&path, &changed, &fixture.public_key, "0.6.12"),
        Err(Error::Signature)
    ));
}

fn manifest_bytes(url: &str, version: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "version": version, "platforms": { "windows-x86_64": { "url": url, "signature": fixture(false).signature } } })).unwrap()
}

#[test]
fn rejects_foreign_hosts_repositories_query_and_unsigned_version() {
    let config = test_config();
    let valid =
        "https://github.com/whitewolf06/fono/releases/download/v0.6.12/Fono_0.6.12_x64-setup.exe";
    assert!(manifest::parse(&manifest_bytes(valid, "0.6.12"), &config).is_ok());
    for url in [
        "http://github.com/whitewolf06/fono/releases/download/v0.6.12/Fono.exe",
        "https://github.com/other/fono/releases/download/v0.6.12/Fono.exe",
        "https://github.com/whitewolf06/fono/releases/download/v0.6.12/Fono.exe?token=x",
        "https://github.com/whitewolf06/fono/releases/download/v0.6.12/dir/Fono.exe",
        "https://evil.example/Fono.exe",
    ] {
        assert!(matches!(
            manifest::parse(&manifest_bytes(url, "0.6.12"), &config),
            Err(Error::UnsafeUrl)
        ));
    }
    assert!(manifest::parse(&manifest_bytes(valid, "0.6.12-beta.1"), &config).is_err());
    assert!(manifest::parse(&manifest_bytes(valid, "0.6.13"), &config).is_err());
    assert!(matches!(manifest::parse(&manifest_bytes("https://github.com/whitewolf06/fono/releases/download/v0.6.11/Fono_0.6.11_x64-setup.exe", "0.6.11"), &config), Err(Error::Version)));
}

#[test]
fn cache_lock_is_os_held_and_stale_lock_file_is_reusable() {
    let directory = TestDirectory::new();
    let lock = CacheLock::acquire(&directory.0).unwrap();
    assert!(matches!(CacheLock::acquire(&directory.0), Err(Error::Busy)));
    #[cfg(windows)]
    {
        assert!(fs::remove_file(directory.0.join("download.lock")).is_err());
        assert!(fs::rename(
            directory.0.join("download.lock"),
            directory.0.join("renamed.lock")
        )
        .is_err());
    }
    drop(lock);
    CacheLock::acquire(&directory.0).unwrap();
}

#[test]
fn partial_state_does_not_persist_signed_redirect_tokens() {
    let directory = TestDirectory::new();
    let cache = Cache::for_release(
        &directory.0,
        &test_release("https://github.com/whitewolf06/fono/releases/download/v0.6.12/Fono.exe"),
    )
    .unwrap();
    cache
        .save_state(&PartialState {
            validator: Some("\"etag\"".into()),
            total: Some(50),
        })
        .unwrap();
    assert_eq!(cache.load_state().total, Some(50));
    let state = fs::read_to_string(cache.partial.parent().unwrap().join("resume.json")).unwrap();
    assert!(!state.contains("https") && !state.contains("signature"));
}

#[test]
fn cancelled_run_does_not_resolve_or_create_cache() {
    let directory = TestDirectory::new();
    let path = directory.0.join("unused");
    let runtime = Bootstrapper::new(test_config(), path.clone()).unwrap();
    assert!(matches!(
        runtime.run(Arc::new(AtomicBool::new(true)), |_| panic!("no progress")),
        Err(Error::Cancelled)
    ));
    assert!(!Path::new(&path).exists());
}

#[cfg(windows)]
#[test]
fn protected_executable_denies_write_delete_and_directory_rename() {
    let directory = TestDirectory::new();
    let lock = CacheLock::acquire(&directory.0).unwrap();
    let release =
        test_release("https://github.com/whitewolf06/fono/releases/download/v0.6.12/Fono.exe");
    let cache = Cache::for_release(&directory.0, &release).unwrap();
    fs::write(&cache.installer, b"protected").unwrap();
    let file = cache::protected_read(&cache.installer).unwrap();
    assert!(fs::write(&cache.installer, b"tampered").is_err());
    assert!(fs::remove_file(&cache.installer).is_err());
    assert!(fs::rename(cache.installer.parent().unwrap(), directory.0.join("moved")).is_err());
    drop(file);
    drop(cache);
    drop(lock);
}

#[test]
fn rejects_hardlinked_cache_files_before_truncating_external_data() {
    let directory = TestDirectory::new();
    let original = directory.0.join("outside.txt");
    let link = directory.0.join("installer.part");
    fs::write(&original, b"do not overwrite").unwrap();
    fs::hard_link(&original, &link).unwrap();
    assert!(matches!(
        cache::open_regular(&link, false),
        Err(Error::Configuration)
    ));
    assert_eq!(fs::read(&original).unwrap(), b"do not overwrite");
    assert!(matches!(
        cache::protected_read(&link),
        Err(Error::Configuration)
    ));
}
