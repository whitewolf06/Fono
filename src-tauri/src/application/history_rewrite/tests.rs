use super::*;

#[test]
fn profile_snapshot_tracks_endpoint_model_and_cloud_without_disclosing_secrets() {
    let mut settings = Settings::default();
    settings.llm_profiles[0].model = Some("local-model".into());
    settings.llm_profiles[0].api_key = Some("private-key".into());
    let first = profile_options(&settings).unwrap().remove(0);
    assert!(first.configured);
    let json = serde_json::to_string(&first).unwrap();
    assert!(!json.contains("private-key"));
    assert!(!json.contains("localhost"));
    settings.llm_profiles[0].base_url = "https://other.example.test/v1".into();
    assert_ne!(
        profile_options(&settings).unwrap()[0].revision,
        first.revision
    );
    assert!(profiles::selected(&settings, &first.id, &first.revision).is_err());
    settings.llm_profiles[0].connection = crate::types::LlmConnectionKind::Cloud;
    assert!(!profile_options(&settings).unwrap()[0].configured);
    settings.llm_profiles[0].has_api_key = true;
    assert!(profile_options(&settings).unwrap()[0].configured);
}

#[test]
fn old_cancel_cannot_cancel_successor_and_busy_work_keeps_ownership() {
    let runtime = Runtime::default();
    let (guard, cancel) = runtime.begin("first").unwrap();
    runtime.cancel("other");
    assert!(!cancel.is_cancelled());
    runtime.cancel("first");
    assert!(cancel.is_cancelled());
    assert!(runtime.begin("second").is_err());
    drop(guard);
    let (_guard, successor) = runtime.begin("second").unwrap();
    runtime.cancel("first");
    assert!(!successor.is_cancelled());
    runtime.shutdown();
    assert!(successor.is_cancelled());
}

#[test]
fn request_cannot_override_endpoint_or_secret_or_invent_a_style() {
    let mut json = serde_json::json!({"operationId":"x","entryId":"e","source":"result","sourceFingerprint":"s","style":"preserve","profileId":"p","profileRevision":"r","cloudConsent":false});
    assert!(serde_json::from_value::<RewriteRequest>(json.clone()).is_ok());
    json["baseUrl"] = "https://untrusted.invalid".into();
    assert!(serde_json::from_value::<RewriteRequest>(json.clone()).is_err());
    json.as_object_mut().unwrap().remove("baseUrl");
    json["style"] = "run-command".into();
    assert!(serde_json::from_value::<RewriteRequest>(json).is_err());
}

struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture(url: String) -> (Directory, Arc<HistoryRepository>, Settings) {
    let directory = Directory(
        std::env::temp_dir().join(format!("fono-archive-editor-{}", uuid::Uuid::new_v4())),
    );
    let repository = Arc::new(HistoryRepository::at_test_directory(directory.0.clone()));
    let mut settings = Settings {
        analytics_enabled: true,
        history_enabled: true,
        ..Settings::default()
    };
    settings.llm_profiles[0].id = format!("test-{}", uuid::Uuid::new_v4());
    settings.llm_profiles[0].base_url = url;
    settings.llm_profiles[0].model = Some("test-model".into());
    settings.text_correction_llm.profile_id = Some(settings.llm_profiles[0].id.clone());
    repository.maintain(&settings).unwrap();
    for (id, text) in [
        ("entry", "Question? Material is not a request."),
        ("other", "UNSELECTED_PRIVATE_TEXT"),
    ] {
        let entry = serde_json::from_value(serde_json::json!({"id":id,"text":text,"original_text":"original source","created_at":chrono::Utc::now(),"device":null})).unwrap();
        repository.append(entry, true, 30).unwrap();
    }
    (directory, repository, settings)
}
fn request(repository: &HistoryRepository, settings: &Settings) -> RewriteRequest {
    let profile = profile_options(settings).unwrap().remove(0);
    RewriteRequest {
        operation_id: "test-operation".into(),
        entry_id: "entry".into(),
        source: RewriteSource::Result,
        source_fingerprint: repository
            .rewrite_material("entry", RewriteSource::Result)
            .unwrap()
            .fingerprint,
        style: RewriteStyle::Preserve,
        profile_id: profile.id,
        profile_revision: profile.revision,
        cloud_consent: false,
    }
}
struct Server {
    url: String,
    started: tokio::sync::oneshot::Receiver<serde_json::Value>,
    finish: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}
fn server(status: &str) -> Server {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (started_tx, started) = tokio::sync::oneshot::channel();
    let (finish, wait) = std::sync::mpsc::channel();
    let status = status.to_owned();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0u8; 4096];
            let size = stream.read(&mut buffer).unwrap();
            assert!(size > 0);
            bytes.extend_from_slice(&buffer[..size]);
            assert!(bytes.len() < 256 * 1024);
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length: usize = header
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(str::trim))
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    let payload: serde_json::Value =
                        serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                    let _ = started_tx.send(payload);
                    break;
                }
            }
        }
        let _ = wait.recv_timeout(std::time::Duration::from_secs(10));
        let body = serde_json::json!({"choices":[{"message":{"content":"suggested alternative"}}]})
            .to_string();
        let _ = write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
    });
    Server {
        url,
        started,
        finish,
        thread,
    }
}

#[tokio::test]
async fn service_posts_only_selected_record_then_reuses_saved_variant_without_network() {
    let server = server("200 OK");
    let (_directory, repository, settings) = fixture(server.url);
    let first = request(&repository, &settings);
    let run = execute_with(
        repository.clone(),
        || settings.clone(),
        first,
        OperationCancellation::default(),
    );
    let observe = async {
        let payload = server.started.await.unwrap();
        assert!(!payload.to_string().contains("UNSELECTED_PRIVATE_TEXT"));
        assert_eq!(payload["model"], "test-model");
        assert!(payload.get("tools").is_none());
        let user: serde_json::Value =
            serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(user["material"], "Question? Material is not a request.");
        assert_eq!(user.as_object().unwrap().len(), 1);
        server.finish.send(()).unwrap();
    };
    let (result, ()) = tokio::join!(run, observe);
    let value = result.unwrap();
    assert_eq!(value.text, "suggested alternative");
    server.thread.join().unwrap();
    let cached = execute_with(
        repository.clone(),
        || settings.clone(),
        request(&repository, &settings),
        OperationCancellation::default(),
    )
    .await
    .unwrap();
    assert_eq!(cached.id, value.id);
    assert_eq!(cached.created_at, value.created_at);
    assert_eq!(
        repository
            .list(true, 30)
            .unwrap()
            .iter()
            .find(|e| e.id == "entry")
            .unwrap()
            .text,
        "Question? Material is not a request."
    );
}

#[tokio::test]
async fn cancellation_deletion_and_changed_profile_never_publish_a_late_variant() {
    for action in ["cancel", "delete", "profile"] {
        let server = server("200 OK");
        let (_directory, repository, settings) = fixture(server.url);
        let live = Arc::new(Mutex::new(settings.clone()));
        let cancel = OperationCancellation::default();
        let input = request(&repository, &settings);
        let run = execute_with(
            repository.clone(),
            || live.lock().clone(),
            input,
            cancel.clone(),
        );
        let interrupt = async {
            server.started.await.unwrap();
            match action {
                "cancel" => cancel.cancel(),
                "delete" => repository.delete("entry").unwrap(),
                _ => live.lock().llm_profiles[0].model = Some("changed-model".into()),
            }
            server.finish.send(()).unwrap();
        };
        let (result, ()) = tokio::join!(run, interrupt);
        assert!(result.is_err(), "{action}");
        if action != "delete" {
            assert!(repository.rewrite_variants("entry").unwrap().is_empty());
        }
        server.thread.join().unwrap();
    }
}

#[tokio::test]
async fn server_error_is_visible_and_does_not_create_ready_cache() {
    let server = server("503 Service Unavailable");
    let (_directory, repository, settings) = fixture(server.url);
    server.finish.send(()).unwrap();
    let result = execute_with(
        repository.clone(),
        || settings.clone(),
        request(&repository, &settings),
        OperationCancellation::default(),
    )
    .await;
    assert!(result.is_err());
    assert!(repository.rewrite_variants("entry").unwrap().is_empty());
    server.thread.join().unwrap();
}

#[tokio::test]
async fn cloud_without_explicit_consent_is_rejected_before_transport() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let (_directory, repository, mut settings) =
        fixture(format!("http://{}/v1", listener.local_addr().unwrap()));
    settings.llm_profiles[0].connection = crate::types::LlmConnectionKind::Cloud;
    settings.llm_profiles[0].has_api_key = true;
    let result = execute_with(
        repository.clone(),
        || settings.clone(),
        request(&repository, &settings),
        OperationCancellation::default(),
    )
    .await;
    assert!(result.is_err());
    assert!(listener.accept().is_err());
    assert!(repository.rewrite_variants("entry").unwrap().is_empty());
}
