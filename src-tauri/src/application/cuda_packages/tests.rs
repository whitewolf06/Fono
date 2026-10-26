use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::fs;
#[cfg(all(windows, not(feature = "cpu-only")))]
use std::sync::atomic::AtomicUsize;

#[derive(Deserialize)]
struct Fixture {
    public_key: String,
    archive_base64: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    kind: String,
    manifest: String,
    signature: String,
}
fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/security.json")).unwrap()
}
fn trust(fixture: &Fixture) -> manifest::Trust {
    manifest::Trust {
        key: fixture.public_key.clone(),
        app_version: "0.0.1".into(),
        protocol_version: 3,
    }
}
fn case<'a>(fixture: &'a Fixture, kind: &str) -> &'a Case {
    fixture.cases.iter().find(|item| item.kind == kind).unwrap()
}
fn verified(fixture: &Fixture, kind: &str) -> manifest::Manifest {
    let case = case(fixture, kind);
    manifest::verify(case.manifest.as_bytes(), &case.signature, &trust(fixture)).unwrap()
}

struct Temporary(
    PathBuf,
    #[cfg(windows)]
    #[allow(dead_code)]
    super::test_subject::Subject,
);
impl Temporary {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("fono-cuda-test-{}", uuid::Uuid::new_v4()));
        #[cfg(windows)]
        {
            let subject = super::test_subject::Subject::create(&path);
            Self(path, subject)
        }
        #[cfg(not(windows))]
        {
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn package(temp: &Temporary, fixture: &Fixture, kind: &str) -> VerifiedPackage {
    let mut stage = storage::Staging::new(&temp.0).unwrap();
    let archive = temp
        .0
        .join(format!("archive-{}.tar.bz2", uuid::Uuid::new_v4()));
    fs::write(&archive, STANDARD.decode(&fixture.archive_base64).unwrap()).unwrap();
    let case = case(fixture, kind);
    let manifest = verified(fixture, kind);
    storage::extract(
        &archive,
        &mut stage,
        &manifest,
        case.manifest.as_bytes(),
        &case.signature,
        &AtomicBool::new(false),
    )
    .unwrap();
    let slot = storage::promote(
        &temp.0,
        &mut stage,
        &manifest::hash(case.manifest.as_bytes()),
    )
    .unwrap();
    storage::verify_slot(&temp.0, &slot, &trust(fixture), &AtomicBool::new(false)).unwrap()
}

#[test]
fn independently_signed_fixture_verifies_but_not_with_the_production_key() {
    let fixture = fixture();
    let case = case(&fixture, "valid");
    assert_eq!(verified(&fixture, "valid").revision, 2);
    let mut other = trust(&fixture);
    other.key = manifest::Trust::production().unwrap().key;
    assert!(matches!(
        manifest::verify(case.manifest.as_bytes(), &case.signature, &other),
        Err(PackageError::Signature)
    ));
}

#[test]
fn payload_signature_global_comment_and_key_tampering_fail_closed() {
    let fixture = fixture();
    let case = case(&fixture, "valid");
    let trust = trust(&fixture);
    let tampered = case.manifest.replace("fono-cuda", "evil-cuda");
    assert!(matches!(
        manifest::verify(tampered.as_bytes(), &case.signature, &trust),
        Err(PackageError::Signature)
    ));
    for line in [1, 3] {
        let text = String::from_utf8(STANDARD.decode(&case.signature).unwrap()).unwrap();
        let mut lines: Vec<_> = text.lines().map(str::to_owned).collect();
        let mut bytes = STANDARD.decode(&lines[line]).unwrap();
        bytes[15] ^= 1;
        lines[line] = STANDARD.encode(bytes);
        assert!(matches!(
            manifest::verify(
                case.manifest.as_bytes(),
                &STANDARD.encode(lines.join("\n")),
                &trust
            ),
            Err(PackageError::Signature)
        ));
    }
    let text = String::from_utf8(STANDARD.decode(&case.signature).unwrap())
        .unwrap()
        .replace("timestamp:1", "timestamp:2");
    assert!(matches!(
        manifest::verify(case.manifest.as_bytes(), &STANDARD.encode(text), &trust),
        Err(PackageError::Signature)
    ));
}

#[test]
fn authenticated_wrong_purpose_platform_app_protocol_schema_and_comment_are_rejected() {
    let fixture = fixture();
    for name in [
        "wrong_purpose",
        "wrong_platform",
        "wrong_app",
        "wrong_protocol",
        "wrong_schema",
        "wrong_comment",
        "duplicate_comment",
    ] {
        let case = case(&fixture, name);
        assert!(
            matches!(
                manifest::verify(case.manifest.as_bytes(), &case.signature, &trust(&fixture)),
                Err(PackageError::Compatibility)
            ),
            "{name}"
        );
    }
}

#[test]
fn authenticated_bad_sizes_names_notices_and_duplicates_are_rejected() {
    let fixture = fixture();
    for name in [
        "zero_revision",
        "archive_oversize",
        "wrong_archive_name",
        "missing_notices",
        "duplicate_file",
        "extra_file",
        "traversal_file",
    ] {
        let case = case(&fixture, name);
        assert!(
            matches!(
                manifest::verify(case.manifest.as_bytes(), &case.signature, &trust(&fixture)),
                Err(PackageError::Manifest)
            ),
            "{name}"
        );
    }
    let mut manifest = verified(&fixture, "valid");
    manifest.files[0].sha256 = "A".repeat(64);
    assert!(manifest::validate(&manifest, &trust(&fixture)).is_err());
    for file in &mut manifest.files {
        file.sha256 = "a".repeat(64);
        file.size = manifest::MAX_ARCHIVE;
    }
    assert!(manifest::validate(&manifest, &trust(&fixture)).is_err());
}

#[test]
fn bounded_signature_decoding_rejects_extra_lines_noncanonical_and_oversized_values() {
    let fixture = fixture();
    let case = case(&fixture, "valid");
    let trust = trust(&fixture);
    let text = String::from_utf8(STANDARD.decode(&case.signature).unwrap()).unwrap();
    for signature in [
        STANDARD.encode(format!("{text}extra\n")),
        "a".repeat(manifest::MAX_SIGNATURE as usize + 1),
        format!("{}=", case.signature),
    ] {
        assert!(manifest::verify(case.manifest.as_bytes(), &signature, &trust).is_err());
    }
    assert!(manifest::verify(
        &vec![b' '; manifest::MAX_MANIFEST as usize + 1],
        &case.signature,
        &trust
    )
    .is_err());
}

#[test]
fn only_the_pinned_github_channel_and_asset_redirect_host_are_allowed() {
    use reqwest::Url;
    assert!(download::asset("0.7.23", "fono-cuda-windows-x86_64.json").is_ok());
    for (version, name) in [
        ("../other", "payload"),
        ("0.7.23", "../payload"),
        ("0.7.23", "evil.exe?secret=x"),
    ] {
        assert!(download::asset(version, name).is_err());
    }
    assert!(download::valid_redirect(
        &Url::parse("https://release-assets.githubusercontent.com/a?token=short-lived").unwrap()
    ));
    for url in [
        "http://github.com/whitewolf06/fono/releases/download/v0.0.1/test",
        "https://evil.example/test",
        "https://127.0.0.1/test",
        "https://github.com/another/fono/releases/download/v0.0.1/test",
        "https://github.com/whitewolf06/fono/releases/download/v0.0.1/test?token=x",
        "https://github.com/whitewolf06/fono/releases/download/v0.0.1/%65vil",
        "https://user:password@release-assets.githubusercontent.com/a",
        "https://release-assets.githubusercontent.com.evil.example/a",
        "https://release-assets.githubusercontent.com/a#fragment",
        "https://release-assets.githubusercontent.com:444/a",
    ] {
        assert!(
            !download::valid_redirect(&Url::parse(url).unwrap()),
            "{url}"
        );
    }
}

#[test]
#[cfg(windows)]
fn verified_slot_preserves_exact_files_and_stays_inactive_until_commit() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let package = package(&temporary, &fixture, "valid");
    assert!(package.worker_path().is_file());
    assert!(storage::active(&temporary.0, &trust(&fixture))
        .unwrap()
        .is_none());
    storage::commit(&temporary.0, &package).unwrap();
    let active = storage::active(&temporary.0, &trust(&fixture))
        .unwrap()
        .unwrap();
    assert_eq!(active, package);
    assert_eq!(active.app_version(), "0.0.1");
    assert_eq!(active.revision(), 2);
    assert_eq!(active.manifest_hash().len(), 64);
}

#[test]
#[cfg(windows)]
fn antirollback_rejects_old_revision_and_different_content_under_the_same_revision() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let package = package(&temporary, &fixture, "valid");
    storage::commit(&temporary.0, &package).unwrap();
    let state = storage::state(&temporary.0).unwrap();
    for name in ["older", "replacement"] {
        let candidate = verified(&fixture, name);
        assert_eq!(
            storage::allow(
                &state,
                &candidate,
                &manifest::hash(case(&fixture, name).manifest.as_bytes())
            ),
            Err(PackageError::Rollback)
        );
    }
    assert!(storage::allow(
        &state,
        &verified(&fixture, "newer"),
        &manifest::hash(case(&fixture, "newer").manifest.as_bytes())
    )
    .is_ok());
    assert!(storage::allow(&state, &package.manifest, &package.hash).is_ok());
}

#[test]
fn malformed_persisted_state_is_not_silently_reset() {
    let temporary = Temporary::new();
    fs::write(temporary.0.join("state.json"), b"{truncated").unwrap();
    assert!(matches!(
        storage::state(&temporary.0),
        Err(PackageError::Storage)
    ));
    fs::write(
        temporary.0.join("state.json"),
        br#"{"schema_version":2,"active":null,"high_water":{}}"#,
    )
    .unwrap();
    assert!(storage::state(&temporary.0).is_err());
}

#[test]
#[cfg(windows)]
fn crash_after_promotion_keeps_previous_pointer_and_new_slot_is_reusable() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let first = package(&temporary, &fixture, "valid");
    storage::commit(&temporary.0, &first).unwrap();
    let second = package(&temporary, &fixture, "newer");
    assert_eq!(
        storage::active(&temporary.0, &trust(&fixture))
            .unwrap()
            .unwrap(),
        first
    );
    assert_eq!(package(&temporary, &fixture, "newer"), second);
    storage::commit(&temporary.0, &second).unwrap();
    assert_eq!(
        storage::active(&temporary.0, &trust(&fixture))
            .unwrap()
            .unwrap(),
        second
    );
    assert!(matches!(
        storage::commit(&temporary.0, &first),
        Err(PackageError::Rollback)
    ));
}

#[test]
#[cfg(windows)]
fn discovery_rejects_tampered_files_and_unexpected_payload_entries() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let package = package(&temporary, &fixture, "valid");
    storage::commit(&temporary.0, &package).unwrap();
    let worker = package.worker_path().to_owned();
    drop(package);
    fs::write(&worker, b"tampered").unwrap();
    assert!(storage::active(&temporary.0, &trust(&fixture)).is_err());
    assert!(fs::write(worker.parent().unwrap().join("extra.dll"), b"unexpected").is_err());
    assert!(storage::active(&temporary.0, &trust(&fixture)).is_err());
}

#[test]
#[cfg(windows)]
fn incompatible_previously_installed_package_is_not_executable() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let package = package(&temporary, &fixture, "valid");
    storage::commit(&temporary.0, &package).unwrap();
    let mut newer_app = trust(&fixture);
    newer_app.app_version = "0.0.2".into();
    assert!(storage::active(&temporary.0, &newer_app).unwrap().is_none());
}

fn malicious_archive(name: &str, kind: u8, contents: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let encoder = bzip2::write::BzEncoder::new(&mut bytes, bzip2::Compression::best());
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_ustar();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::new(kind));
    header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
    header.set_cksum();
    archive.append(&header, contents).unwrap();
    let encoder = archive.into_inner().unwrap();
    encoder.finish().unwrap();
    bytes
}

#[test]
fn extraction_rejects_traversal_absolute_ads_case_aliases_links_and_tar_extensions() {
    let fixture = fixture();
    let manifest = verified(&fixture, "valid");
    let case = case(&fixture, "valid");
    let data = b"not executable";
    for (name, kind) in [
        ("../outside.dll", b'0'),
        ("/outside.dll", b'0'),
        ("C:\\outside.dll", b'0'),
        ("cublas64_13.dll:stream", b'0'),
        ("CUBLAS64_13.DLL", b'0'),
        ("fono-stt-cuda-worker.exe", b'1'),
        ("fono-stt-cuda-worker.exe", b'2'),
        ("fono-stt-cuda-worker.exe", b'5'),
        ("fono-stt-cuda-worker.exe", b'x'),
        ("fono-stt-cuda-worker.exe", b'L'),
    ] {
        let temporary = Temporary::new();
        let mut stage = storage::Staging::new(&temporary.0).unwrap();
        let source = temporary.0.join("malicious.tar.bz2");
        fs::write(&source, malicious_archive(name, kind, data)).unwrap();
        assert!(
            storage::extract(
                &source,
                &mut stage,
                &manifest,
                case.manifest.as_bytes(),
                &case.signature,
                &AtomicBool::new(false)
            )
            .is_err(),
            "{name} {kind}"
        );
        assert!(!temporary.0.join("outside.dll").exists());
        assert!(!temporary.0.join("state.json").exists());
    }
}

#[test]
fn extraction_rejects_missing_duplicate_and_corrupted_members() {
    let fixture = fixture();
    let manifest = verified(&fixture, "valid");
    let case = case(&fixture, "valid");
    let bytes = STANDARD.decode(&fixture.archive_base64).unwrap();
    let mut source = tar::Archive::new(bzip2::read::BzDecoder::new(bytes.as_slice()));
    let mut members = Vec::new();
    for entry in source.entries().unwrap() {
        let mut entry = entry.unwrap();
        let mut contents = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut contents).unwrap();
        members.push((entry.header().clone(), contents));
    }
    for attack in ["missing", "duplicate", "corrupt"] {
        let temporary = Temporary::new();
        let mut stage = storage::Staging::new(&temporary.0).unwrap();
        let path = temporary.0.join("attack.tar.bz2");
        let encoder = bzip2::write::BzEncoder::new(
            fs::File::create(&path).unwrap(),
            bzip2::Compression::best(),
        );
        let mut archive = tar::Builder::new(encoder);
        for (index, (header, content)) in members.iter().enumerate() {
            if attack == "missing" && index == 0 {
                continue;
            }
            let mut bytes = content.clone();
            if attack == "corrupt" && index == 0 {
                bytes[0] ^= 1;
            }
            archive.append(header, bytes.as_slice()).unwrap();
            if attack == "duplicate" && index == 0 {
                archive.append(header, bytes.as_slice()).unwrap();
            }
        }
        archive.into_inner().unwrap().finish().unwrap();
        assert!(
            storage::extract(
                &path,
                &mut stage,
                &manifest,
                case.manifest.as_bytes(),
                &case.signature,
                &AtomicBool::new(false)
            )
            .is_err(),
            "{attack}"
        );
    }
}

#[test]
fn cancellation_prevents_extraction_and_cleanup_removes_only_its_own_staging() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let abandoned = temporary.0.join("staging/previous-interrupted-operation");
    fs::create_dir_all(&abandoned).unwrap();
    fs::write(abandoned.join("retained"), b"retained").unwrap();
    let mut stage = storage::Staging::new(&temporary.0).unwrap();
    let stage_path = stage.path.clone();
    let source = temporary.0.join("source.tar.bz2");
    fs::write(&source, STANDARD.decode(&fixture.archive_base64).unwrap()).unwrap();
    let case = case(&fixture, "valid");
    assert_eq!(
        storage::extract(
            &source,
            &mut stage,
            &verified(&fixture, "valid"),
            case.manifest.as_bytes(),
            &case.signature,
            &AtomicBool::new(true)
        ),
        Err(PackageError::Cancelled)
    );
    drop(stage);
    assert!(!stage_path.exists());
    assert_eq!(fs::read(abandoned.join("retained")).unwrap(), b"retained");
}

#[cfg(windows)]
#[test]
fn installed_guard_prevents_payload_write_delete_and_directory_replacement() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let package = package(&temporary, &fixture, "valid");
    assert!(fs::OpenOptions::new()
        .write(true)
        .open(package.worker_path())
        .is_err());
    assert!(fs::remove_file(package.worker_path()).is_err());
    assert!(fs::rename(
        package.worker_path().parent().unwrap(),
        temporary.0.join("replaced")
    )
    .is_err());
    let clone = package.clone();
    let worker = package.worker_path().to_owned();
    drop(package);
    assert!(fs::OpenOptions::new().write(true).open(&worker).is_err());
    drop(clone);
    assert!(fs::OpenOptions::new().write(true).open(worker).is_ok());
}

#[cfg(windows)]
#[test]
fn sealed_payload_blocks_unsigned_additions_and_keeps_the_root_state_writable() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let package = package(&temporary, &fixture, "valid");
    package.validate_layout().unwrap();
    let unexpected = package
        .worker_path()
        .parent()
        .unwrap()
        .join("unsigned-dependency.dll");
    // The OWNER RIGHTS ACL covers creation that share-mode guards alone do
    // not prevent, including app-local VCRUNTIME/MSVCP DLL hijacking.
    assert!(fs::write(&unexpected, b"must never participate in worker loading").is_err());
    assert!(!sealing::owner_can_reset_acl(package.worker_path().parent().unwrap()).unwrap());
    fs::write(temporary.0.join("state-probe.tmp"), b"root still writable").unwrap();
    package.validate_layout().unwrap();
}

#[cfg(windows)]
#[test]
fn interrupted_sealed_staging_is_removed_without_weakening_a_published_acl() {
    let temporary = Temporary::new();
    sealing::parent_modify(&temporary.0).unwrap();
    let fixture = fixture();
    let mut stage = storage::Staging::new(&temporary.0).unwrap();
    let stage_path = stage.path.clone();
    let archive = temporary.0.join("source.tar.bz2");
    fs::write(&archive, STANDARD.decode(&fixture.archive_base64).unwrap()).unwrap();
    let case = case(&fixture, "valid");
    storage::extract(
        &archive,
        &mut stage,
        &verified(&fixture, "valid"),
        case.manifest.as_bytes(),
        &case.signature,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(fs::write(stage.path.join("payload/extra.dll"), b"blocked").is_err());
    drop(stage);
    assert!(!stage_path.exists());
    // Cleanup may not depend on a broad DELETE_CHILD grant inherited from
    // the runner's Temp directory; the fixture's parent grants only Modify.
    fs::remove_file(&archive).unwrap();
    fs::remove_dir(temporary.0.join("staging")).unwrap();
    fs::remove_dir(&temporary.0).unwrap();
}

#[cfg(windows)]
#[test]
fn staging_and_promotion_parents_cannot_be_renamed_for_a_junction_swap() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let mut stage = storage::Staging::new(&temporary.0).unwrap();
    let source_parent = temporary.0.join("staging");
    // A rename is the first step of replacing a checked parent with a
    // junction. These deterministic attempts fail while actual work owns it.
    assert!(fs::rename(&temporary.0, temporary.0.with_extension("root-swap")).is_err());
    assert!(fs::rename(&source_parent, temporary.0.join("staging-swap")).is_err());
    assert!(fs::rename(&stage.path, temporary.0.join("uuid-swap")).is_err());
    let archive = temporary.0.join("source.tar.bz2");
    fs::write(&archive, STANDARD.decode(&fixture.archive_base64).unwrap()).unwrap();
    let case = case(&fixture, "valid");
    storage::extract(
        &archive,
        &mut stage,
        &verified(&fixture, "valid"),
        case.manifest.as_bytes(),
        &case.signature,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(fs::rename(stage.path.join("payload"), temporary.0.join("payload-swap")).is_err());
    let old_uuid = stage.path.clone();
    let slot = storage::promote(
        &temporary.0,
        &mut stage,
        &manifest::hash(case.manifest.as_bytes()),
    )
    .unwrap();
    assert!(!old_uuid.exists());
    // Before verify_slot owns its own full ancestor chain, Staging must keep
    // the destination parent stable across the promotion-to-verification gap.
    assert!(fs::rename(temporary.0.join("slots"), temporary.0.join("slots-swap")).is_err());
    assert!(fs::rename(&source_parent, temporary.0.join("staging-swap")).is_err());
    let package = storage::verify_slot(
        &temporary.0,
        &slot,
        &trust(&fixture),
        &AtomicBool::new(false),
    )
    .unwrap();
    // A newly created object at the vacated UUID is not this operation's
    // staging. Drop must never touch that namespace after promotion.
    fs::create_dir(&old_uuid).unwrap();
    fs::write(old_uuid.join("unrelated"), b"retained").unwrap();
    drop(stage);
    assert_eq!(fs::read(old_uuid.join("unrelated")).unwrap(), b"retained");
    assert!(fs::rename(temporary.0.join("slots"), temporary.0.join("slots-swap")).is_err());
    drop(package);
    fs::rename(
        temporary.0.join("slots"),
        temporary.0.join("slots-after-release"),
    )
    .unwrap();
    fs::rename(&source_parent, temporary.0.join("staging-after-release")).unwrap();
}

#[cfg(windows)]
#[test]
fn existing_destination_reuses_verified_slot_and_cleans_only_its_owned_staging() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let first = package(&temporary, &fixture, "valid");
    let mut stage = storage::Staging::new(&temporary.0).unwrap();
    let old_uuid = stage.path.clone();
    let archive = temporary.0.join("existing-source.tar.bz2");
    fs::write(&archive, STANDARD.decode(&fixture.archive_base64).unwrap()).unwrap();
    let case = case(&fixture, "valid");
    storage::extract(
        &archive,
        &mut stage,
        &verified(&fixture, "valid"),
        case.manifest.as_bytes(),
        &case.signature,
        &AtomicBool::new(false),
    )
    .unwrap();
    let slot = storage::promote(&temporary.0, &mut stage, first.manifest_hash()).unwrap();
    assert_eq!(
        slot.join("payload/fono-stt-cuda-worker.exe").as_path(),
        first.worker_path()
    );
    assert!(old_uuid.exists());
    drop(stage);
    assert!(!old_uuid.exists());
    first.validate_layout().unwrap();
}

#[cfg(windows)]
#[test]
fn cross_process_install_lock_is_exclusive_and_released_on_drop() {
    let temporary = Temporary::new();
    let lock = storage::install_lock(&temporary.0).unwrap();
    assert!(storage::install_lock(&temporary.0).is_err());
    drop(lock);
    assert!(storage::install_lock(&temporary.0).is_ok());
}

#[cfg(windows)]
#[test]
fn junction_ancestors_are_rejected_without_touching_the_target() {
    let temporary = Temporary::new();
    let outside = Temporary::new();
    let junction = temporary.0.join("junction");
    let result = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "junction fixture creation failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let attempt = storage::install_lock(&junction.join("cuda-packages"));
    assert!(attempt.is_err());
    assert!(!outside.0.join("cuda-packages").exists());
    fs::remove_dir(&junction).unwrap();
}

#[cfg(all(windows, not(feature = "cpu-only")))]
#[test]
fn single_flight_cancel_is_generation_scoped_and_ticket_survives_ipc_drop() {
    let temporary = Temporary::new();
    let service = CudaPackageService::new(temporary.0.clone());
    let operation = service.begin("first".into()).unwrap();
    assert!(service.begin("duplicate".into()).is_err());
    service.cancel("stale");
    assert!(!operation.cancel.load(Ordering::Acquire));
    service.cancel("first");
    assert!(operation.cancel.load(Ordering::Acquire));
    assert_eq!(service.snapshot().phase, CudaPackagePhase::Cancelling);
    assert!(service.begin("too-early".into()).is_err());
    drop(operation);
    let next = service.begin("next".into()).unwrap();
    service.cancel("first");
    assert!(!next.cancel.load(Ordering::Acquire));
}

#[cfg(all(windows, not(feature = "cpu-only")))]
#[test]
fn probe_and_activation_failures_never_commit_but_cancel_after_commit_cannot_undo_it() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let package = package(&temporary, &fixture, "valid");
    let service = CudaPackageService::new(temporary.0.clone());
    let probes = AtomicUsize::new(0);
    let operation = service.begin("probe-failure".into()).unwrap();
    assert!(service
        .activate_prepared(
            &operation,
            &package,
            |_| {
                probes.fetch_add(1, Ordering::Relaxed);
                Err(AppError::Config("synthetic source error".into()))
            },
            |_, _| panic!("activation ran after failed probe")
        )
        .is_err());
    drop(operation);
    assert!(!temporary.0.join("state.json").exists());
    let operation = service.begin("cancelled-activation".into()).unwrap();
    assert!(service
        .activate_prepared(
            &operation,
            &package,
            |_| Ok(()),
            |_, commit| {
                service.cancel("cancelled-activation");
                commit()
            }
        )
        .is_err());
    drop(operation);
    assert!(!temporary.0.join("state.json").exists());
    let operation = service.begin("accepted".into()).unwrap();
    service
        .activate_prepared(
            &operation,
            &package,
            |_| {
                probes.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
            |_, commit| {
                commit()?;
                service.cancel("accepted");
                Ok(())
            },
        )
        .unwrap();
    assert!(!operation.cancel.load(Ordering::Acquire));
    assert_eq!(service.snapshot().phase, CudaPackagePhase::Installed);
    drop(operation);
    assert_eq!(
        storage::active(&temporary.0, &trust(&fixture))
            .unwrap()
            .unwrap(),
        package
    );
    assert_eq!(probes.load(Ordering::Relaxed), 2);
}

#[cfg(feature = "cpu-only")]
#[test]
fn cpu_profile_rejects_installation_before_io_and_never_discovers_a_gpu_slot() {
    let temporary = Temporary::new();
    let root = temporary.0.join("must-stay-absent");
    let service = CudaPackageService::new(root.clone());
    assert!(service.begin("forbidden".into()).is_err());
    assert_eq!(service.snapshot().phase, CudaPackagePhase::Unavailable);
    assert!(active_package(&root).unwrap().is_none());
    assert!(!root.exists());
}

#[test]
fn public_status_never_includes_worker_paths_keys_tokens_or_stderr() {
    let temporary = Temporary::new();
    let service = CudaPackageService::new(temporary.0.clone());
    let json = serde_json::to_string(&service.snapshot()).unwrap();
    assert!(!json.contains(temporary.0.to_str().unwrap()));
    assert!(!json.contains("https://"));
    assert!(!json.contains("public_key"));
    assert_eq!(
        check_cancel(&AtomicBool::new(true)),
        Err(PackageError::Cancelled)
    );
}

#[test]
fn cuda_recommendation_excludes_software_and_other_hardware_vendors() {
    assert!(is_hardware_nvidia(0x10de, false));
    assert!(!is_hardware_nvidia(0x10de, true));
    assert!(!is_hardware_nvidia(0x1002, false));
    assert!(!is_hardware_nvidia(0x8086, false));
}

#[cfg(all(windows, not(feature = "cpu-only")))]
#[test]
fn delayed_startup_refresh_never_overwrites_a_newly_committed_generation() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let old = package(&temporary, &fixture, "valid");
    let newer = package(&temporary, &fixture, "newer");
    let service = CudaPackageService::new(temporary.0.clone());
    let stale_ticket = service.shared.data.lock().epoch;
    let operation = service.begin("new-install".into()).unwrap();
    service
        .activate_prepared(&operation, &newer, |_| Ok(()), |_, commit| commit())
        .unwrap();
    drop(operation);
    service
        .publish_refresh(stale_ticket, Some(false), Ok(Some(old)))
        .unwrap();
    assert_eq!(service.verified_active().unwrap(), newer);
    service
        .publish_refresh(
            stale_ticket,
            None,
            Err(AppError::Config("old failed read".into())),
        )
        .unwrap();
    assert_eq!(service.snapshot().phase, CudaPackagePhase::Installed);
    assert_eq!(service.verified_active().unwrap(), newer);
}
