use super::*;
use openssl::{
    asn1::{Asn1Integer, Asn1Time},
    bn::BigNum,
    hash::MessageDigest,
    pkey::{PKey, Private},
    rsa::Rsa,
    x509::{
        extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
        X509NameBuilder, X509,
    },
};
use std::os::unix::fs::PermissionsExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Files(std::path::PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("fono-tls-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn settings(&self) -> TlsSettings {
        TlsSettings {
            cert: self.0.join("cert.pem"),
            key: self.0.join("key.pem"),
        }
    }
    fn write(&self, cert: &X509, key: &PKey<Private>) {
        fs::write(self.settings().cert, cert.to_pem().unwrap()).unwrap();
        fs::write(self.settings().key, key.private_key_to_pem_pkcs8().unwrap()).unwrap();
        fs::set_permissions(self.settings().key, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn root() -> (X509, PKey<Private>) {
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "Fono test CA").unwrap();
    let name = name.build();
    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    cert.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(1).unwrap()).unwrap())
        .unwrap();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(&name).unwrap();
    cert.set_pubkey(&key).unwrap();
    cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    cert.set_not_after(&Asn1Time::days_from_now(2).unwrap())
        .unwrap();
    cert.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
        .unwrap();
    cert.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()
            .unwrap(),
    )
    .unwrap();
    cert.sign(&key, MessageDigest::sha256()).unwrap();
    (cert.build(), key)
}

fn leaf(ca: &X509, ca_key: &PKey<Private>, serial: u32, expired: bool) -> (X509, PKey<Private>) {
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "localhost").unwrap();
    let name = name.build();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    cert.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(serial).unwrap()).unwrap())
        .unwrap();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(ca.subject_name()).unwrap();
    cert.set_pubkey(&key).unwrap();
    cert.set_not_before(&Asn1Time::from_unix(now - 60).unwrap())
        .unwrap();
    cert.set_not_after(&Asn1Time::from_unix(now + if expired { -1 } else { 86400 }).unwrap())
        .unwrap();
    cert.append_extension(BasicConstraints::new().critical().build().unwrap())
        .unwrap();
    cert.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
        .unwrap();
    cert.append_extension(
        KeyUsage::new()
            .digital_signature()
            .key_encipherment()
            .build()
            .unwrap(),
    )
    .unwrap();
    cert.append_extension(
        SubjectAlternativeName::new()
            .dns("localhost")
            .ip("127.0.0.1")
            .build(&cert.x509v3_context(Some(ca), None))
            .unwrap(),
    )
    .unwrap();
    cert.sign(ca_key, MessageDigest::sha256()).unwrap();
    (cert.build(), key)
}

async fn client(
    address: std::net::SocketAddr,
    ca: &X509,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, std::io::Error> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(ca.to_der().unwrap()))
        .unwrap();
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(
            "localhost".try_into().unwrap(),
            tokio::net::TcpStream::connect(address).await?,
        )
        .await
}

async fn get(stream: &mut tokio_rustls::client::TlsStream<tokio::net::TcpStream>) {
    get_path(stream, "/healthz").await;
}

async fn get_path(stream: &mut tokio_rustls::client::TlsStream<tokio::net::TcpStream>, path: &str) {
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        header.push(byte[0]);
        assert!(header.len() < 4096);
    }
    assert!(header.starts_with(b"HTTP/1.1 200"));
    let mut body = [0; 2];
    stream.read_exact(&mut body).await.unwrap();
    assert_eq!(&body, b"ok");
}

#[test]
fn invalid_or_missing_material_fails_without_paths_or_native_errors() {
    let settings = TlsSettings {
        cert: "/private-certificate-path.pem".into(),
        key: "/private-key-path.pem".into(),
    };
    let error = load_config(&settings).unwrap_err();
    assert_eq!(error, TlsError::File);
    assert!(!error.to_string().contains("private-"));
}

#[test]
fn load_rejects_mismatch_expired_certificate_and_unsafe_key_permissions() {
    let files = Files::new();
    let (ca, ca_key) = root();
    let (cert, key) = leaf(&ca, &ca_key, 2, false);
    files.write(&cert, &key);
    load_config(&files.settings()).unwrap();
    fs::set_permissions(files.settings().key, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        load_config(&files.settings()).unwrap_err(),
        TlsError::Permissions
    );
    let (expired, other_key) = leaf(&ca, &ca_key, 3, true);
    files.write(&cert, &other_key);
    assert_eq!(
        load_config(&files.settings()).unwrap_err(),
        TlsError::Mismatch
    );
    files.write(&expired, &other_key);
    assert_eq!(
        load_config(&files.settings()).unwrap_err(),
        TlsError::Validity
    );
    fs::write(
        files.settings().key,
        other_key
            .private_key_to_pem_pkcs8_passphrase(
                openssl::symm::Cipher::aes_256_cbc(),
                b"secret-password",
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        load_config(&files.settings()).unwrap_err(),
        TlsError::PrivateKey
    );
}

#[tokio::test]
async fn certificate_reload_is_transactional_and_keeps_existing_tls_connections() {
    let files = Files::new();
    let (ca, ca_key) = root();
    let (first, first_key) = leaf(&ca, &ca_key, 2, false);
    files.write(&first, &first_key);
    let state = Arc::new(TlsState::new(files.settings()).unwrap());
    let activity = ServerActivity::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let finish = Arc::new(tokio::sync::Notify::new());
    let slow_started = started.clone();
    let slow_finish = finish.clone();
    let router = Router::new()
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .route(
            "/slow",
            axum::routing::get(move || {
                let started = slow_started.clone();
                let finish = slow_finish.clone();
                async move {
                    started.notify_one();
                    finish.notified().await;
                    "ok"
                }
            }),
        );
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let host = tokio::spawn(serve(
        router,
        listener,
        state.clone(),
        activity.clone(),
        async {
            let _ = stop_rx.await;
        },
    ));
    let mut existing = client(address, &ca).await.unwrap();
    get(&mut existing).await;
    let pending = tokio::spawn(async move {
        get_path(&mut existing, "/slow").await;
        existing
    });
    started.notified().await;
    let (second, second_key) = leaf(&ca, &ca_key, 3, false);
    files.write(&first, &second_key);
    assert_eq!(
        state.reload(activity.clone()).await.unwrap_err(),
        TlsError::Mismatch
    );
    let still_old = client(address, &ca).await.unwrap();
    assert_eq!(
        still_old.get_ref().1.peer_certificates().unwrap()[0].as_ref(),
        first.to_der().unwrap()
    );
    drop(still_old);
    files.write(&second, &second_key);
    state.reload(activity.clone()).await.unwrap();
    let mut renewed = client(address, &ca).await.unwrap();
    assert_eq!(
        renewed.get_ref().1.peer_certificates().unwrap()[0].as_ref(),
        second.to_der().unwrap()
    );
    get(&mut renewed).await;
    assert!(!pending.is_finished());
    finish.notify_one();
    let mut existing = pending.await.unwrap();
    get(&mut existing).await;
    let (wrong_ca, _) = root();
    assert!(client(address, &wrong_ca).await.is_err());
    drop((renewed, existing));
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), host)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(activity.active(), 0);
}
