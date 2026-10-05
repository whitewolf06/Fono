use crate::download_test_support::{client, response, Server};
use crate::{
    cache::{Cache, PartialState},
    core_tests::{test_config, test_release, TestDirectory},
    download, *,
};
use std::{fs, sync::atomic::Ordering, thread, time::Duration};

#[test]
fn range_at_size_limit_never_has_an_end_before_start() {
    let maximum = crate::manifest::MAX_INSTALLER_BYTES;
    assert_eq!(download::range_header(maximum), format!("bytes={maximum}-"));
    assert_eq!(
        download::range_header(maximum - 1),
        format!("bytes={}-{}", maximum - 1, maximum - 1)
    );
    assert_eq!(download::range_header(4), "bytes=4-8388611");
}

fn fetch(
    server: &Server,
    cache: &Cache,
    cancel: &AtomicBool,
    progress: &impl Fn(Progress),
) -> Result<()> {
    download::installer(
        &client(),
        &test_config(),
        &test_release(&server.url),
        cache,
        cancel,
        progress,
    )
}

#[test]
fn resumes_interrupted_download_with_range_and_if_range() {
    let server = Server::new(vec![
        response("200 OK", "Content-Length: 10\r\nETag: \"v1\"\r\n", "0123"),
        response(
            "206 Partial Content",
            "Content-Length: 6\r\nContent-Range: bytes 4-9/10\r\nETag: \"v1\"\r\n",
            "456789",
        ),
    ]);
    let directory = TestDirectory::new();
    let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
    fetch(&server, &cache, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(fs::read(&cache.partial).unwrap(), b"0123456789");
    let requests = server.requests();
    assert!(requests[1].to_lowercase().contains("range: bytes=4-"));
    assert!(requests[1].to_lowercase().contains("if-range: \"v1\""));
}

#[test]
fn restarts_when_server_ignores_range() {
    let server = Server::new(vec![response(
        "200 OK",
        "Content-Length: 10\r\nETag: \"v2\"\r\n",
        "0123456789",
    )]);
    let directory = TestDirectory::new();
    let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
    fs::write(&cache.partial, b"old!").unwrap();
    cache
        .save_state(&PartialState {
            validator: Some("\"v1\"".into()),
            total: Some(10),
        })
        .unwrap();
    fetch(&server, &cache, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(fs::read(&cache.partial).unwrap(), b"0123456789");
    assert!(server.requests()[0]
        .to_lowercase()
        .contains("range: bytes=4-"));
}

#[test]
fn wrong_206_or_416_resets_partial_before_retry() {
    for failed in [
        response(
            "206 Partial Content",
            "Content-Length: 5\r\nContent-Range: bytes 5-9/10\r\n",
            "56789",
        ),
        response(
            "416 Range Not Satisfiable",
            "Content-Length: 0\r\nContent-Range: bytes */3\r\n",
            "",
        ),
    ] {
        let server = Server::new(vec![
            failed,
            response("200 OK", "Content-Length: 10\r\n", "0123456789"),
        ]);
        let directory = TestDirectory::new();
        let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
        fs::write(&cache.partial, b"0123").unwrap();
        cache
            .save_state(&PartialState {
                validator: Some("\"v1\"".into()),
                total: Some(10),
            })
            .unwrap();
        fetch(&server, &cache, &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!(fs::read(&cache.partial).unwrap(), b"0123456789");
        assert!(!server.requests()[1].to_lowercase().contains("range:"));
    }
}

#[test]
fn complete_416_preserves_data_for_signature_verification() {
    let server = Server::new(vec![response(
        "416 Range Not Satisfiable",
        "Content-Length: 0\r\nContent-Range: bytes */10\r\n",
        "",
    )]);
    let directory = TestDirectory::new();
    let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
    fs::write(&cache.partial, b"0123456789").unwrap();
    cache
        .save_state(&PartialState {
            validator: Some("\"v1\"".into()),
            total: Some(10),
        })
        .unwrap();
    fetch(&server, &cache, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(fs::read(&cache.partial).unwrap(), b"0123456789");
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn cancelled_partial_download_can_be_resumed_on_next_run() {
    let server = Server::new(vec![
        response("200 OK", "Content-Length: 10\r\nETag: \"v1\"\r\n", "0123"),
        response(
            "206 Partial Content",
            "Content-Length: 6\r\nContent-Range: bytes 4-9/10\r\nETag: \"v1\"\r\n",
            "456789",
        ),
    ]);
    let directory = TestDirectory::new();
    let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(matches!(
        fetch(&server, &cache, &cancel, &|event| {
            if event.downloaded_bytes == 4 {
                cancel.store(true, Ordering::Release);
            }
        }),
        Err(Error::Cancelled)
    ));
    assert_eq!(fs::read(&cache.partial).unwrap(), b"0123");
    cancel.store(false, Ordering::Release);
    fetch(&server, &cache, &cancel, &|_| {}).unwrap();
    assert_eq!(fs::read(&cache.partial).unwrap(), b"0123456789");
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn rejects_redirect_to_untrusted_host_before_following() {
    let server = Server::new(vec![response(
        "302 Found",
        "Content-Length: 0\r\nLocation: https://evil.example/payload.exe\r\n",
        "",
    )]);
    let directory = TestDirectory::new();
    let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
    assert!(matches!(
        fetch(&server, &cache, &AtomicBool::new(false), &|_| {}),
        Err(Error::UnsafeUrl)
    ));
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn rejects_oversized_response_without_writing_payload() {
    let server = Server::new(vec![response(
        "200 OK",
        "Content-Length: 536870913\r\n",
        "",
    )]);
    let directory = TestDirectory::new();
    let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
    assert!(matches!(
        fetch(&server, &cache, &AtomicBool::new(false), &|_| {}),
        Err(Error::Size)
    ));
    assert!(!cache.partial.exists());
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn retries_transient_status_from_original_download_url() {
    for status in [
        "500 Internal Server Error",
        "403 Forbidden",
        "429 Too Many Requests",
    ] {
        let server = Server::new(vec![
            response(status, "Content-Length: 0\r\n", ""),
            response("200 OK", "Content-Length: 10\r\n", "0123456789"),
        ]);
        let directory = TestDirectory::new();
        let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
        fetch(&server, &cache, &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!(fs::read(&cache.partial).unwrap(), b"0123456789");
        let requests = server.requests();
        assert!(requests
            .iter()
            .all(|request| request.starts_with("GET /Fono.exe HTTP/1.1")));
    }
}

#[test]
fn active_transfer_cannot_outlive_overall_download_budget() {
    let server = Server::new(vec![response(
        "200 OK",
        "Content-Length: 10\r\nETag: \"v1\"\r\n",
        "0123456789",
    )]);
    let directory = TestDirectory::new();
    let cache = Cache::for_release(&directory.0, &test_release(&server.url)).unwrap();
    let result = download::installer_with_budget(
        &client(),
        &test_config(),
        &test_release(&server.url),
        &cache,
        &AtomicBool::new(false),
        &|progress| {
            if progress.downloaded_bytes > 0 {
                thread::sleep(Duration::from_millis(150));
            }
        },
        Duration::from_millis(100),
    );
    assert!(matches!(result, Err(Error::Network)));
    assert_eq!(fs::read(&cache.partial).unwrap(), b"0123456789");
    assert_eq!(server.requests().len(), 1);
}
