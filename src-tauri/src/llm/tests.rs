use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

#[test]
fn processing_readiness_checks_model_endpoint_and_actual_required_key() {
    for (url, model, key, requires_key, ready) in [
        ("http://127.0.0.1:1234/v1", Some("model"), None, false, true),
        (
            "https://api.example.test/v1",
            Some("model"),
            Some("key"),
            true,
            true,
        ),
        (
            "https://api.example.test/v1",
            Some("model"),
            None,
            true,
            false,
        ),
        (
            "https://api.example.test/v1",
            Some("model"),
            Some("  "),
            true,
            false,
        ),
        ("http://127.0.0.1:1234/v1", None, None, false, false),
        ("http://127.0.0.1:1234/v1", Some("  "), None, false, false),
        ("", Some("model"), None, false, false),
        ("bad-url", Some("model"), None, false, false),
        ("file:///tmp/model", Some("model"), None, false, false),
    ] {
        let client = LlmClient::new(url, model.map(str::to_owned), key.map(str::to_owned));
        assert_eq!(client.is_configured(requires_key), ready, "{url}");
    }
}

#[test]
fn metrics_only_speech_payload_cannot_serialize_private_findings() {
    let analysis =
        crate::speech_metrics::analyze_russian_speech("СекретНимб СекретНимб, ну, как бы");
    assert!(!analysis.findings.is_empty());
    let input = serde_json::to_value(SpeechAnalysisRequest {
        metrics: SpeechNumericMetrics::from(&analysis),
        findings: None,
        original_text: None,
    })
    .unwrap();
    let metrics = input["metrics"].as_object().unwrap();
    assert_eq!(metrics.len(), 6);
    assert!(metrics.values().all(serde_json::Value::is_number));
    assert!(input.get("findings").is_none());
    assert!(input.get("original_text").is_none());
    assert!(!input.to_string().contains("Секрет"));
    assert!(!input.to_string().contains("fragment"));
}

#[test]
fn explicit_findings_scope_is_separate_from_numeric_metrics() {
    let analysis = crate::speech_metrics::analyze_russian_speech("ну ну");
    let input = serde_json::to_value(SpeechAnalysisRequest {
        metrics: SpeechNumericMetrics::from(&analysis),
        findings: Some(&analysis.findings),
        original_text: None,
    })
    .unwrap();
    assert!(input["metrics"].get("findings").is_none());
    assert!(input["findings"].is_array());
    assert!(input.get("original_text").is_none());
}

#[test]
fn numeric_only_recommendation_cannot_reference_unshared_local_findings() {
    let result = parse_speech_recommendation(
            r#"{"summary":"Потренируйте паузы","recommendations":[{"title":"Паузы","observation":"Высокая плотность маркеров","exercise":"Пауза между мыслями","finding_indexes":[0,1]}]}"#,
            0,
        ).unwrap();
    assert!(result.recommendations[0].finding_indexes.is_empty());
}

#[test]
fn period_recommendation_requires_actionable_priorities() {
    assert!(parse_period_recommendation(r#"{"summary":"итог","priorities":[]}"#).is_err());
    assert!(parse_period_recommendation(
        r#"{"summary":"итог","priorities":[{"title":"Пауза","basis":"","exercise":"Пауза"}]}"#
    )
    .is_err());
    let result = parse_period_recommendation(r#"{"summary":"итог","priorities":[{"title":"Пауза","basis":"4 маркера на 100 слов","exercise":"Делайте паузу перед новой мыслью"}]}"#).unwrap();
    assert_eq!(result.priorities.len(), 1);
}

fn response_with(content: &str) -> ChatResponse {
    ChatResponse {
        choices: vec![ChatChoice {
            message: ChatChoiceMessage {
                content: content.to_string(),
            },
        }],
    }
}

fn test_server(status: &str, content_type: &str, body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
    let address = listener
        .local_addr()
        .expect("read local test server address");
    let status = status.to_string();
    let content_type = content_type.to_string();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept one client");
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        // Drain the complete request before closing a response. On Windows,
        // unread POST bytes can reset the connection and hide the intended 503.
        let mut request = Vec::new();
        loop {
            let mut chunk = [0_u8; 4_096];
            let size = stream.read(&mut chunk).expect("read local test request");
            assert!(size > 0, "request ended before its declared body");
            request.extend_from_slice(&chunk[..size]);
            assert!(
                request.len() <= 256 * 1024,
                "test request exceeds its limit"
            );
            if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length: usize = header
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:").map(str::trim))
                    .unwrap_or("0")
                    .parse()
                    .expect("valid request content length");
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
        stream
            .write_all(headers.as_bytes())
            .and_then(|_| stream.write_all(&body))
            .expect("write local test response");
    });

    format!("http://{address}/v1")
}

#[test]
fn configured_postprocessing_keeps_server_failure_visible() {
    let base_url = test_server(
        "503 Service Unavailable",
        "application/json",
        b"{}".to_vec(),
    );
    let client = LlmClient::new(base_url, Some("test-model".into()), None);
    assert!(client.is_configured(false));
    let error = run_async(client.process_preset(
        "test transcript",
        crate::types::TextPreset::Clean,
        None,
        None,
    ))
    .expect_err("A configured server failure must not be treated as unfinished setup");
    assert!(
        error.to_string().contains("503 Service Unavailable"),
        "{error}"
    );
}

fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create test runtime")
        .block_on(future)
}

#[test]
fn anonymous_model_discovery_sends_only_get_without_auth_or_text() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut bytes = [0_u8; 4096];
        let size = stream.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..size]).into_owned();
        let body = r#"{"data":[{"id":"local-small"},{"id":"local-large"}]}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        request
    });
    let client = LlmClient::new(format!("http://{address}/v1"), None, None);
    assert_eq!(
        run_async(client.list_models()).unwrap(),
        vec!["local-small", "local-large"]
    );
    let request = server.join().unwrap();
    assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"));
    assert!(!request.to_ascii_lowercase().contains("authorization"));
    assert_eq!(request.split_once("\r\n\r\n").unwrap().1, "");
}

#[test]
fn response_content_trims_valid_content() {
    assert_eq!(
        response_content(response_with("  result  ")).unwrap(),
        "result"
    );
}

#[test]
fn response_content_rejects_empty_content() {
    assert!(response_content(response_with(" \n ")).is_err());
    assert!(response_content(ChatResponse { choices: vec![] }).is_err());
}

#[test]
fn http_client_is_shared_between_requests() {
    let first = shared_http_client().expect("initialize shared client");
    let second = shared_http_client().expect("reuse shared client");
    assert!(std::ptr::eq(first, second));
}

#[test]
fn http_fault_injection_preserves_non_success_status() {
    let base_url = test_server(
        "503 Service Unavailable",
        "application/json",
        b"{}".to_vec(),
    );
    let client = LlmClient::new(base_url, Some("test-model".to_string()), None);

    let error = run_async(client.list_models()).expect_err("503 must fail");
    assert!(error.to_string().contains("503 Service Unavailable"));
}

#[test]
fn http_fault_injection_rejects_malformed_json() {
    let base_url = test_server("200 OK", "application/json", b"not-json".to_vec());
    let client = LlmClient::new(base_url, Some("test-model".to_string()), None);

    let error = run_async(client.list_models()).expect_err("malformed JSON must fail");
    assert!(error.to_string().contains("parse /models"));
}

#[test]
fn http_fault_injection_rejects_oversized_response() {
    let base_url = test_server(
        "200 OK",
        "application/json",
        vec![b'x'; MAX_RESPONSE_BYTES + 1],
    );
    let client = LlmClient::new(base_url, Some("test-model".to_string()), None);

    let error = run_async(client.list_models()).expect_err("oversized response must fail");
    assert!(error.to_string().contains("response exceeds"));
}

#[test]
fn speech_recommendation_keeps_only_known_finding_indexes() {
    let recommendation = parse_speech_recommendation(
            r#"{"summary":"Коротко","recommendations":[{"title":"Пауза","observation":"Есть вводные","exercise":"Сделайте паузу","finding_indexes":[0,4]}]}"#,
            1,
        )
        .expect("valid response");

    assert_eq!(recommendation.recommendations[0].finding_indexes, vec![0]);
}
