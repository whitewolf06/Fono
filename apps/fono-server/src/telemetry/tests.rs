use super::*;

#[derive(Clone)]
struct Buffer(Arc<Mutex<Vec<u8>>>);
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn structured_records_have_an_allowlisted_schema_and_respect_level() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (telemetry, guard) =
        Observability::with_writer(LogLevel::Warn, Buffer(bytes.clone())).unwrap();
    telemetry.event(LogLevel::Info, Phase::Listening);
    telemetry.http_completed(200, Duration::from_millis(5));
    telemetry.http_completed(401, Duration::from_millis(12));
    telemetry.inference(Outcome::WorkerFailed, Duration::from_secs(1), 16_000);
    guard.finish();
    let source = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    let records: Vec<serde_json::Value> = source
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["status"], 401);
    assert_eq!(records[1]["outcome"], "worker_failed");
    for record in records {
        for key in record.as_object().unwrap().keys() {
            assert!(
                [
                    "timestamp",
                    "level",
                    "event",
                    "phase",
                    "status",
                    "elapsed_us",
                    "audio_ms",
                    "outcome"
                ]
                .contains(&key.as_str()),
                "unexpected log field {key}"
            );
        }
    }
}

#[test]
fn histogram_buckets_and_measurements_follow_real_observations() {
    let telemetry = Observability::default();
    {
        let _waiting = telemetry.waiting(QueueClass::Interactive);
        let mut body = String::new();
        telemetry.render(&mut body);
        assert!(body.contains("fono_stt_waiting{priority=\"interactive\"} 1\n"));
    }
    telemetry.http_completed(429, Duration::from_millis(100));
    telemetry.inference(Outcome::Ok, Duration::from_millis(500), 16_000);
    telemetry.inference(Outcome::Preempted, Duration::from_millis(250), 16_000);
    let mut body = String::new();
    telemetry.render(&mut body);
    assert!(body.contains("fono_stt_waiting{priority=\"interactive\"} 0\n"));
    assert!(body.contains("fono_http_responses_total{status=\"429\"} 1\n"));
    assert!(body.contains("fono_stt_decode_seconds_count 2\n"));
    assert!(body.contains("fono_stt_realtime_factor_count 1\n"));
    assert!(body.contains("fono_stt_realtime_factor_bucket{le=\"0.5\"} 1\n"));
    assert!(body.contains("fono_stt_decode_seconds_bucket{le=\"+Inf\"} 2\n"));
    assert!(!body.contains("fono_gpu_memory_total_bytes{device="));
    assert!(body.contains("fono_gpu_inventory_available 0\n"));
}

#[test]
fn driver_values_are_preserved_and_unknown_memory_is_omitted() {
    let telemetry = Observability::default();
    telemetry.observe_gpu(Some(vec![crate::backends::CudaDevice {
        index: 2,
        name: "private GPU name".into(),
        memory_total_mb: 8192,
        memory_free_mb: 6000,
        driver_version: "595.84".into(),
    }]));
    let mut body = String::new();
    telemetry.render(&mut body);
    assert!(body.contains("fono_gpu_memory_free_bytes{device=\"cuda:2\"} 6291456000\n"));
    assert!(!body.contains("private GPU"));
    telemetry.observe_gpu(None);
    body.clear();
    telemetry.render(&mut body);
    assert!(!body.contains("fono_gpu_memory_free_bytes{device="));
}

#[test]
fn blocked_log_consumer_drops_events_without_blocking_inference_threads() {
    struct BlockingWriter {
        entered: mpsc::Sender<()>,
        resume: mpsc::Receiver<()>,
    }
    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let _ = self.entered.send(());
            let _ = self.resume.recv();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (telemetry, guard) = Observability::with_writer(
        LogLevel::Debug,
        BlockingWriter {
            entered: entered_tx,
            resume: resume_rx,
        },
    )
    .unwrap();
    telemetry.event(LogLevel::Info, Phase::Starting);
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    for _ in 0..300 {
        telemetry.http_completed(200, Duration::from_millis(1));
    }
    assert!(telemetry.dropped.load(Ordering::Relaxed) >= 44);
    drop(resume_tx);
    guard.finish();
}

#[test]
fn vulkan_allocation_budget_is_not_reported_as_adapter_free_vram() {
    let telemetry = Observability::default();
    telemetry.observe_vulkan_gpu(Some(fono_stt::vulkan::VulkanDevice {
        index: 0,
        name: "private GPU name".into(),
        uuid: "00112233445566778899aabbccddeeff".into(),
        driver: "fixture".into(),
        discrete: true,
        supported: true,
        memory_total_bytes: 8192,
        memory_free_bytes: None,
        memory_budget_free_bytes: Some(8000),
    }));
    let mut output = String::new();
    telemetry.render(&mut output);
    assert!(output.contains("fono_gpu_memory_total_bytes{device=\"vulkan\"} 8192\n"));
    assert!(!output.contains("fono_gpu_memory_free_bytes{device="));
    assert!(!output.contains("private GPU name"));
}
