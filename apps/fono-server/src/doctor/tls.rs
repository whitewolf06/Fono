use super::Report;
use crate::config::ServerConfig;

pub(super) fn inspect(config: &ServerConfig, report: &mut Report) {
    let Some(tls) = &config.tls else {
        report.check(
            "tls_runtime",
            false,
            true,
            "Native TLS is not configured",
            "",
        );
        return;
    };
    // The file checks are useful preflight even while SRV-11 is pending.
    report.check("tls_runtime", true, false, "Native TLS serving is unavailable in this build",
        "Use a TLS reverse proxy or encrypted tunnel; remove the native tls section until SRV-11 is implemented.");
    #[cfg(unix)]
    inspect_unix(tls, report);
    #[cfg(not(unix))]
    {
        let _ = tls;
        report.check(
            "tls_files",
            true,
            false,
            "Certificate inspection is available in the Ubuntu build",
            "Run doctor in the Ubuntu server image to inspect the configured TLS files.",
        );
    }
}

#[cfg(unix)]
fn inspect_unix(tls: &crate::config::TlsSettings, report: &mut Report) {
    use openssl::{asn1::Asn1Time, pkey::PKey, x509::X509};
    let certificate = bounded_file(&tls.cert)
        .ok()
        .and_then(|bytes| X509::stack_from_pem(&bytes).ok())
        .and_then(|mut chain| {
            if chain.is_empty() {
                None
            } else {
                Some(chain.remove(0))
            }
        });
    report.check(
        "tls_certificate",
        true,
        certificate.is_some(),
        if certificate.is_some() {
            "PEM certificate parsed"
        } else {
            "Certificate is missing, unreadable, oversized or invalid PEM"
        },
        "Provide a readable PEM certificate chain in tls.cert.",
    );
    let key_bytes = bounded_file(&tls.key).ok();
    let key = key_bytes
        .as_ref()
        .and_then(|bytes| PKey::private_key_from_pem_passphrase(bytes, b"").ok());
    report.check("tls_private_key", true, key.is_some(),
        if key.is_some() { "PEM private key parsed without prompting" } else { "Private key is missing, unreadable, encrypted or invalid PEM" },
        "Provide the matching unencrypted PEM private key in tls.key with restricted file permissions.");
    let permissions_ok = std::fs::metadata(&tls.key).is_ok_and(|metadata| {
        use std::os::unix::fs::MetadataExt;
        metadata.is_file()
            && metadata.mode() & 0o027 == 0
            && (metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() })
    });
    report.check("tls_key_permissions", true, permissions_ok,
        if permissions_ok { "Private key ownership and permissions are restricted" } else { "Private key ownership or permissions are unsafe" },
        "Use an owner-controlled regular key file with mode 0600, 0400 or a trusted read-only group (0640).");
    if let Some(cert) = &certificate {
        let valid = Asn1Time::days_from_now(0).ok().is_some_and(|now| {
            cert.not_before()
                .compare(&now)
                .is_ok_and(|order| !order.is_gt())
                && cert
                    .not_after()
                    .compare(&now)
                    .is_ok_and(|order| order.is_gt())
        });
        report.check("tls_certificate_time", true, valid,
            if valid { "Leaf certificate validity period includes the current time" } else { "Certificate is expired, not yet valid or has invalid dates" },
            "Renew the certificate and check the host clock; certificate trust is managed by your TLS endpoint.");
    }
    if let (Some(cert), Some(key)) = (certificate, key) {
        let matches = cert.public_key().is_ok_and(|public| public.public_eq(&key));
        report.check(
            "tls_key_match",
            true,
            matches,
            if matches {
                "Certificate and private key match"
            } else {
                "Certificate and private key do not match"
            },
            "Configure the certificate and private key from the same key pair.",
        );
    }
}

#[cfg(unix)]
fn bounded_file(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    if !std::fs::metadata(path)?.is_file() {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut bytes = Vec::new();
    file.take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > 1_048_576 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use openssl::{
        asn1::Asn1Time,
        hash::MessageDigest,
        pkey::{PKey, Private},
        rsa::Rsa,
        x509::{X509NameBuilder, X509},
    };
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    struct Files(PathBuf);
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn certificate(key: &PKey<Private>, expired: bool) -> Vec<u8> {
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "private-doctor-certificate.invalid")
            .unwrap();
        let name = name.build();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let mut builder = X509::builder().unwrap();
        builder.set_version(2).unwrap();
        builder.set_subject_name(&name).unwrap();
        builder.set_issuer_name(&name).unwrap();
        builder.set_pubkey(key).unwrap();
        builder
            .set_not_before(&Asn1Time::from_unix(now - 172_800).unwrap())
            .unwrap();
        builder
            .set_not_after(
                &Asn1Time::from_unix(now + if expired { -86_400 } else { 86_400 }).unwrap(),
            )
            .unwrap();
        builder.sign(key, MessageDigest::sha256()).unwrap();
        builder.build().to_pem().unwrap()
    }

    #[test]
    fn tls_checks_dates_pair_permissions_and_never_exposes_pem_or_names() {
        let files =
            Files(std::env::temp_dir().join(format!("fono-doctor-tls-{}", uuid::Uuid::new_v4())));
        fs::create_dir(&files.0).unwrap();
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let cert_path = files.0.join("private-certificate.pem");
        let key_path = files.0.join("private-key.pem");
        fs::write(&cert_path, certificate(&key, false)).unwrap();
        fs::write(&key_path, key.private_key_to_pem_pkcs8().unwrap()).unwrap();
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = super::super::tests::config();
        config.tls = Some(crate::config::TlsSettings {
            cert: cert_path.clone(),
            key: key_path.clone(),
        });
        let mut report = Report::new();
        inspect(&config, &mut report);
        assert!(report
            .checks
            .iter()
            .filter(|check| check.name != "tls_runtime")
            .all(|check| check.ok));
        assert!(!report.required_checks_pass()); // Serving still waits for SRV-11.
        let encoded = serde_json::to_string(&report).unwrap();
        for secret in [
            "private-key.pem",
            "BEGIN PRIVATE KEY",
            "private-doctor-certificate.invalid",
        ] {
            assert!(!encoded.contains(secret));
        }
        fs::write(&cert_path, certificate(&key, true)).unwrap();
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o644)).unwrap();
        let other = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        fs::write(&key_path, other.private_key_to_pem_pkcs8().unwrap()).unwrap();
        let mut report = Report::new();
        inspect(&config, &mut report);
        for name in [
            "tls_certificate_time",
            "tls_key_match",
            "tls_key_permissions",
        ] {
            assert!(
                !report
                    .checks
                    .iter()
                    .find(|check| check.name == name)
                    .unwrap()
                    .ok
            );
        }
    }

    #[test]
    fn encrypted_or_non_regular_keys_fail_without_prompting() {
        let files = Files(
            std::env::temp_dir().join(format!("fono-doctor-encrypted-{}", uuid::Uuid::new_v4())),
        );
        fs::create_dir(&files.0).unwrap();
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let path = files.0.join("key.pem");
        fs::write(
            &path,
            key.private_key_to_pem_pkcs8_passphrase(
                openssl::symm::Cipher::aes_256_cbc(),
                b"private-passphrase",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(PKey::private_key_from_pem_passphrase(&bounded_file(&path).unwrap(), b"").is_err());
        assert!(bounded_file(&files.0).is_err());
        let fifo = files.0.join("not-a-key.fifo");
        use std::os::unix::ffi::OsStrExt;
        let native = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: native is a valid NUL-terminated path in this test's directory.
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        assert!(bounded_file(&fifo).is_err());
    }
}
