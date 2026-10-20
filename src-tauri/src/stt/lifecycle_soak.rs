//! Opt-in subsystem soak: production capture/coordinator/scheduler and transport.
//! Only the physical audio adapter is replaced; no desktop or microphone is used.
use super::*;
use crate::pipeline::Pipeline;
use fono_core::{OperationPhase, OperationSource, TerminalReason};
use serde_json::{json, Value};

#[path = "lifecycle_soak_config.rs"]
mod config;
use crate::test_process as win32;
use config::{Config, MODEL_SHA256};

type Result<T> = std::result::Result<T, String>;
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
struct MockAudio {
    pcm: Arc<Vec<i16>>,
    active: AtomicBool,
}
impl crate::audio::AudioRecorder<crate::audio::RecordingWriter, AppError> for MockAudio {
    fn start(
        &self,
        _: Option<&str>,
        writer: crate::audio::RecordingWriter,
        _: Arc<AtomicBool>,
        _: Arc<std::sync::atomic::AtomicU32>,
        maximum: usize,
    ) -> AppResult<()> {
        if self.active.swap(true, Ordering::AcqRel) {
            return Err(AppError::Busy("mock capture already active".into()));
        }
        writer.lock().append(&self.pcm, maximum);
        Ok(())
    }
    fn stop(&self) -> AppResult<()> {
        self.active.store(false, Ordering::Release);
        Ok(())
    }
    fn shutdown(&self) {
        self.active.store(false, Ordering::Release);
    }
}
#[derive(Default)]
struct Latencies {
    start: Vec<f64>,
    capture_stop: Vec<f64>,
    scheduler: Vec<f64>,
    cancel: Vec<f64>,
    reload: Vec<f64>,
}
fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn summary(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |p: f64| sorted[((sorted.len() as f64 * p).ceil() as usize).saturating_sub(1)];
    json!({ "count": sorted.len(), "p50": percentile(0.50), "p95": percentile(0.95), "p99": percentile(0.99), "maximum": sorted.last() })
}
fn start_worker(config: &Config, cleanup: &Arc<WorkerCleanupSupervisor>) -> Result<WorkerSession> {
    // BackendKind::Cpu is checked against the real worker's actual protocol
    // handshake and ModelLoaded response by the unmodified production client.
    WorkerSession::start(
        config.worker.clone(),
        BackendKind::Cpu,
        &config.model,
        cleanup.clone(),
        None,
    )
    .map_err(|e| e.to_string())
}
fn child_pid(worker: &WorkerSession) -> u32 {
    worker.child.process.lock().id()
}
fn cleanup_confirmed(child: &Arc<WorkerChild>, cleanup: &WorkerCleanupSupervisor) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !child.exited() || cleanup.is_pending() {
        require(
            Instant::now() < deadline,
            "owned worker/stdio cleanup did not finish within 10 seconds",
        )?;
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
fn shutdown(worker: &mut WorkerSession, cleanup: &WorkerCleanupSupervisor) -> Result<()> {
    let child = worker.child.clone();
    let _ = worker.shutdown();
    cleanup_confirmed(&child, cleanup)
}
fn checkpoint(
    report: &mut Value,
    name: &str,
    cycle: usize,
    worker: Option<&WorkerSession>,
) -> Result<()> {
    let parent = win32::resources(std::process::id())?;
    let child = worker.map(|w| win32::resources(child_pid(w))).transpose()?;
    report["resource_samples"]
        .as_array_mut()
        .ok_or("invalid report sample array")?
        .push(json!({ "phase": name, "cycle": cycle, "parent": parent, "worker": child }));
    Ok(())
}

/// The test-only counter advances only after the production stdin writer
/// confirms the complete Transcribe frame. The controller then observes CPU
/// activity and cancels this operation; native-kernel entry is not instrumented.
fn active_cancel(
    pipeline: &Arc<Pipeline>,
    operation: u64,
    worker: &mut WorkerSession,
    samples: &[i16],
    cancellation: &OperationCancellation,
) -> Result<(f64, bool)> {
    let pid = child_pid(worker);
    let baseline = win32::cpu_seconds(pid)?;
    let written_frames = worker.written_frames.clone();
    let before_write = written_frames.load(Ordering::Acquire);
    let pipeline_for_cancel = pipeline.clone();
    let controller = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let transported = loop {
            if written_frames.load(Ordering::Acquire) > before_write {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(1));
        };
        let observed = loop {
            match win32::cpu_seconds(pid) {
                Ok(cpu) if cpu > baseline => break true,
                Err(_) => break false,
                _ if Instant::now() >= deadline => break false,
                _ => thread::sleep(Duration::from_millis(2)),
            }
        };
        let cancelled_at = Instant::now();
        let retired = pipeline_for_cancel.cancel_for(operation).is_some();
        (transported, observed, retired, cancelled_at)
    });
    let result = worker.transcribe_cancellable(samples, "en", cancellation);
    let (transported, observed, retired, cancelled_at) = controller
        .join()
        .map_err(|_| "cancellation controller panicked")?;
    let cancel_ms = milliseconds(cancelled_at);
    require(
        transported,
        "active cancellation did not observe a fully written Transcribe frame",
    )?;
    require(
        observed,
        "active cancellation did not observe real CPU worker activity",
    )?;
    require(
        retired && cancellation.is_cancelled(),
        "operation cancellation was not propagated",
    )?;
    require(
        matches!(result, Err(AppError::Cancelled(_))),
        "active real-worker request did not return Cancelled",
    )?;
    Ok((cancel_ms, observed))
}

fn capture_cycle(
    pipeline: &Arc<Pipeline>,
    worker: &mut WorkerSession,
    active: bool,
    active_pcm: &[i16],
    previous: u64,
    latency: &mut Latencies,
) -> Result<u64> {
    let start = Instant::now();
    let operation = pipeline
        .start_recording_from(None, OperationSource::Diagnostics)
        .map_err(|e| e.to_string())?;
    latency.start.push(milliseconds(start));
    let cancellation = pipeline
        .cancellation(operation)
        .ok_or("missing coordinator cancellation")?;
    if previous != 0 {
        require(
            pipeline.cancel_for(previous).is_none(),
            "stale Cancel retired a successor",
        )?;
        require(
            pipeline
                .stop_recording_for(previous)
                .map_err(|e| e.to_string())?
                .is_none(),
            "stale Stop collected successor audio",
        )?;
        require(
            pipeline.is_recording() && !cancellation.is_cancelled(),
            "stale callback changed active capture",
        )?;
    }
    let stopped = Instant::now();
    let samples = pipeline
        .stop_recording_for(operation)
        .map_err(|e| e.to_string())?
        .ok_or("capture buffer missing")?;
    latency.capture_stop.push(milliseconds(stopped));
    require(
        !samples.is_empty(),
        "mock public WAV did not traverse the production bounded capture writer",
    )?;
    require(
        pipeline
            .sync_operation_state_for(
                operation,
                crate::types::PipelineState::Transcribing,
                TerminalReason::Completed,
            )
            .is_some(),
        "coordinator did not enter transcribing",
    )?;
    require(
        pipeline
            .current_operation()
            .is_some_and(|o| o.phase == OperationPhase::Transcribing),
        "phase projection mismatch",
    )?;
    let scheduler = pipeline.scheduler();
    let waiting = Instant::now();
    let permit = scheduler
        .acquire(true, &cancellation)
        .map_err(|e| e.to_string())?;
    latency.scheduler.push(milliseconds(waiting));
    let cancelled = Instant::now();
    if active {
        let (elapsed, _) = active_cancel(
            pipeline,
            operation,
            worker,
            active_pcm,
            &permit.cancellation,
        )?;
        latency.cancel.push(elapsed);
    } else {
        require(
            pipeline.cancel_for(operation).is_some(),
            "coordinator did not cancel capture operation",
        )?;
        let result = worker.transcribe_cancellable(&samples, "en", &permit.cancellation);
        require(
            matches!(result, Err(AppError::Cancelled(_))),
            "pretransport cancellation reached worker inference",
        )?;
        latency.cancel.push(milliseconds(cancelled));
    }
    require(
        scheduler.is_busy(),
        "scheduler released actual-work permit prematurely",
    )?;
    drop(permit);
    require(
        !scheduler.is_busy(),
        "scheduler leaked reservation/permit after cancellation",
    )?;
    require(
        pipeline.current_operation().is_none() && !pipeline.is_recording(),
        "pipeline stayed active after cancelled work",
    )?;
    Ok(operation)
}

fn inference(worker: &mut WorkerSession, samples: &[i16]) -> Result<Value> {
    let started = Instant::now();
    let transcript = worker
        .transcribe(samples, "en")
        .map_err(|e| e.to_string())?;
    require(
        transcript.device.as_deref() == Some("CPU"),
        "inference did not report CPU",
    )?;
    // No recognized text is retained. The licensed public WAV is the only input.
    Ok(
        json!({ "wall_ms": milliseconds(started), "reported_inference_seconds": transcript.transcribe_secs, "audio_seconds": transcript.audio_secs, "nonempty_result": !transcript.text.trim().is_empty(), "backend": transcript.device }),
    )
}
fn growth_gate(before: &Value, after: &Value) -> Value {
    let number = |v: &Value, key: &str| v[key].as_u64().unwrap_or(u64::MAX);
    let bytes = number(before, "private_bytes");
    let allowance = (bytes / 20).max(10 * 1024 * 1024);
    json!({
        "handle_delta": number(after, "handles") as i64 - number(before, "handles") as i64,
        "thread_delta": number(after, "threads") as i64 - number(before, "threads") as i64,
        "private_bytes_delta": number(after, "private_bytes") as i64 - bytes as i64,
        "private_bytes_allowance": allowance,
        "handles_pass": number(after, "handles") <= number(before, "handles"),
        "threads_pass": number(after, "threads") <= number(before, "threads"),
        "private_bytes_pass": number(after, "private_bytes") <= bytes.saturating_add(allowance)
    })
}
fn run(config: &Config, report: &mut Value) -> Result<()> {
    win32::protect_descendants()?;
    win32::watch_runner(config.runner_pid, config.timeout_seconds)?;
    let pcm = Arc::new(config::read_pcm(&config.wav)?);
    let active_pcm: Vec<i16> = pcm.iter().copied().cycle().take(16_000 * 30).collect();
    let pipeline = Arc::new(Pipeline::new_with_test_audio_owner(Box::new(MockAudio {
        pcm: pcm.clone(),
        active: AtomicBool::new(false),
    })));
    let cleanup = Arc::new(WorkerCleanupSupervisor::default());
    let mut latency = Latencies::default();
    checkpoint(report, "cold_pre_worker", 0, None)?;
    let loading = Instant::now();
    let mut worker = start_worker(config, &cleanup)?;
    report["first_session_load_ms"] = json!(milliseconds(loading));
    report["first_inference"] = inference(&mut worker, &pcm)?;
    let warm = [inference(&mut worker, &pcm)?, inference(&mut worker, &pcm)?];
    report["warm_inference"] = json!({ "measurements": warm, "wall_ms": summary(&warm.iter().filter_map(|v| v["wall_ms"].as_f64()).collect::<Vec<_>>()) });
    let mut previous = 0;
    for _ in 0..20 {
        previous = capture_cycle(
            &pipeline,
            &mut worker,
            false,
            &active_pcm,
            previous,
            &mut Latencies::default(),
        )?;
    }
    // Retain the cold snapshot but accept resources only against post-warmup
    // states. Stable OS/runtime initialization is distinct from ongoing drift.
    shutdown(&mut worker, &cleanup)?;
    drop(worker);
    thread::sleep(Duration::from_millis(250));
    checkpoint(report, "warm_no_worker_baseline", 0, None)?;
    let measured_loading = Instant::now();
    let mut worker = start_worker(config, &cleanup)?;
    report["measured_session_load_ms"] = json!(milliseconds(measured_loading));
    report["measured_session_warmup_inference"] = inference(&mut worker, &pcm)?;
    checkpoint(report, "warm_baseline", 0, Some(&worker))?;
    for cycle in 1..=config.cycles {
        let active = cycle % config.active_cancel_every == 0;
        previous = capture_cycle(
            &pipeline,
            &mut worker,
            active,
            &active_pcm,
            previous,
            &mut latency,
        )?;
        report["completed"]["start_cancel_cycles"] = json!(cycle);
        let key = if active {
            "active_worker_cancellations"
        } else {
            "pretransport_worker_cancellations"
        };
        report["completed"][key] = json!(report["completed"][key].as_u64().unwrap_or(0) + 1);
        if worker.ping().is_err() {
            shutdown(&mut worker, &cleanup)?;
            worker = start_worker(config, &cleanup)?;
            report["completed"]["cancellation_reloads"] = json!(
                report["completed"]["cancellation_reloads"]
                    .as_u64()
                    .unwrap_or(0)
                    + 1
            );
        }
        if cycle % (config.cycles / 10).max(1) == 0 {
            checkpoint(report, "capture_cycles", cycle, Some(&worker))?;
        }
    }
    for fault in 1..=config.faults {
        let child = worker.child.clone();
        let pid = child_pid(&worker);
        require(
            matches!(child.try_wait(), Ok(None)),
            "fault target exited before the injected kill",
        )?;
        child.kill().map_err(|e| e.to_string())?;
        report["completed"]["fault_kills"] = json!(fault);
        require(
            worker.ping().is_err(),
            "killed CPU worker answered health successfully",
        )?;
        shutdown(&mut worker, &cleanup)?;
        require(child.exited(), "faulted owned worker was not reaped")?;
        report["worker_faults"].as_array_mut().ok_or("missing worker-fault receipts")?.push(json!({ "index": fault, "owned_pid": pid, "alive_before_kill": true, "confirmed_exit": true, "exit_code": child.try_wait().map_err(|e| e.to_string())?.and_then(|v| v.code()) }));
        report["completed"]["fault_reaps"] = json!(fault);
        let reload = Instant::now();
        worker = start_worker(config, &cleanup)?;
        worker.ping().map_err(|e| e.to_string())?;
        latency.reload.push(milliseconds(reload));
        report["completed"]["fault_restarts"] = json!(fault);
        if fault % (config.faults / 10).max(1) == 0 {
            checkpoint(report, "worker_faults", fault, Some(&worker))?;
        }
    }
    report["after_faults_inference"] = inference(&mut worker, &pcm)?;
    let idle_started = Instant::now();
    while idle_started.elapsed().as_secs() < config.idle_seconds {
        let remaining = config.idle_seconds - idle_started.elapsed().as_secs();
        thread::sleep(Duration::from_secs(remaining.min(10)));
        worker.ping().map_err(|e| e.to_string())?;
        checkpoint(
            report,
            "idle",
            idle_started.elapsed().as_secs() as usize,
            Some(&worker),
        )?;
    }
    report["measured_idle_seconds"] = json!(idle_started.elapsed().as_secs_f64());
    checkpoint(report, "final_loaded", config.cycles, Some(&worker))?;
    shutdown(&mut worker, &cleanup)?;
    drop(worker);
    pipeline.shutdown();
    // Keep the same inert Pipeline/job/watchdog objects alive on both sides of
    // the warm no-worker comparison; they drop only after the measured run.
    require(!cleanup.is_pending(), "cleanup supervisor still owns work")?;
    thread::sleep(Duration::from_millis(250));
    checkpoint(report, "final_shutdown", config.cycles, None)?;
    report["completed"]["all_owned_children_exited"] = json!(true);
    report["latency_ms"] = json!({ "capture_start": summary(&latency.start), "capture_stop": summary(&latency.capture_stop), "scheduler_acquire_uncontended": summary(&latency.scheduler), "cancel_to_request_return": summary(&latency.cancel), "worker_fault_reload": summary(&latency.reload) });
    let samples = report["resource_samples"]
        .as_array()
        .ok_or("missing samples")?;
    let phase = |name| {
        samples
            .iter()
            .find(|v| v["phase"] == name)
            .ok_or("missing resource phase")
    };
    let loaded_growth = growth_gate(
        &phase("warm_baseline")?["parent"],
        &phase("final_loaded")?["parent"],
    );
    let shutdown_growth = growth_gate(
        &phase("warm_no_worker_baseline")?["parent"],
        &phase("final_shutdown")?["parent"],
    );
    let loaded_worker_growth = growth_gate(
        &phase("warm_baseline")?["worker"],
        &phase("final_loaded")?["worker"],
    );
    report["resource_gates"] = json!({ "loaded_parent": loaded_growth, "shutdown_parent": shutdown_growth, "loaded_worker_comparison": loaded_worker_growth, "handle_tolerance": 0, "thread_tolerance": 0, "private_bytes_rule": "baseline + max(5%, 10 MiB)", "monotonic_growth": monotonic_growth(samples), "worker_comparison_scope": "different freshly restarted owned helper; descriptive, parent lifecycle budgets are gates" });
    let gates_pass = [loaded_growth, shutdown_growth].iter().all(|gate| {
        ["handles_pass", "threads_pass", "private_bytes_pass"]
            .iter()
            .all(|key| gate[key] == true)
    });
    require(
        gates_pass,
        "measured process resources exceeded the declared soak budget",
    )?;
    Ok(())
}
fn monotonic_growth(samples: &[Value]) -> Value {
    let measured: Vec<_> = samples
        .iter()
        .filter(|v| {
            matches!(
                v["phase"].as_str(),
                Some(
                    "warm_baseline" | "capture_cycles" | "worker_faults" | "idle" | "final_loaded"
                )
            )
        })
        .collect();
    let grows = |key: &str| {
        let values: Vec<_> = measured
            .iter()
            .filter_map(|v| v["parent"][key].as_u64())
            .collect();
        values.len() >= 3
            && values.windows(2).all(|v| v[1] >= v[0])
            && values.last() > values.first()
    };
    json!({ "samples": measured.len(), "parent_handles": grows("handles"), "parent_threads": grows("threads"), "parent_private_bytes": grows("private_bytes"), "definition": "all checkpoints nondecreasing and final greater than warm baseline; reported separately from bounded endpoint gates" })
}

#[test]
#[ignore = "explicit subsystem soak configuration required; excluded from ordinary native matrices"]
fn cpu_worker_lifecycle_soak() {
    let config = Config::read()
        .expect("use scripts/lifecycle-soak.mjs with an explicit CPU worker and hashes");
    let started = Instant::now();
    let mut report = json!({
        "schema": 1, "kind": "fono-production-subsystem-lifecycle-soak", "status": "running",
        "app_version": env!("CARGO_PKG_VERSION"), "profile": "native unit-test executable; CPU helper profile recorded by runner",
        "limits": { "start_cancel_cycles": config.cycles, "worker_faults": config.faults, "active_cancel_every": config.active_cancel_every, "idle_seconds": config.idle_seconds },
        "inputs": { "worker_sha256": config.worker_sha256, "model_sha256": MODEL_SHA256, "wav_sha256": config.wav_sha256, "sample_rate": 16000, "backend": "cpu" },
        "fault_injection": { "phase": "loaded idle helper after a successful health request", "method": "kill only the owned live Child, detect transport failure, reap, reopen and reload with production WorkerSession", "automatic_recovery_router_tested": false },
        "warmup": { "capture_cycles": 20, "initial_session_inferences": 3, "measured_session_inferences_before_baseline": 1, "excluded_from_completed_counts": true, "cold_snapshot": "cold_pre_worker is descriptive only", "shutdown_gate_baseline": "warm_no_worker_baseline after confirmed warmup child shutdown and 250ms settling" },
        "completed": { "start_cancel_cycles": 0, "pretransport_worker_cancellations": 0, "active_worker_cancellations": 0, "cancellation_reloads": 0, "fault_kills": 0, "fault_reaps": 0, "fault_restarts": 0, "all_owned_children_exited": false },
        "resource_samples": [], "worker_faults": [],
        "scope": { "production": ["Pipeline capture ownership and bounded writer", "OperationCoordinator and cancellation token", "SttScheduler permit and reservation", "WorkerSession protocol, deadlines and cleanup supervisor", "real CPU Whisper helper, model load and public-WAV inference"], "mock": ["physical AudioRecorder only; WAV copied synchronously into real writer"], "unmeasured": ["microphone and CPAL callback/overflow", "Tauri IPC/windows/hotkey/clipboard", "LLM/insertion latency", "automatic SttEngine recovery routing", "GPU drivers", "release installation and owner UAT"], "physical_dropped_frames": null, "physical_queue_overflow": null, "cold_cache_claim": false }
    });
    let result = run(&config, &mut report);
    report["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
    report["status"] = json!(if result.is_ok() { "passed" } else { "failed" });
    report["error"] = json!(result.as_ref().err());
    if let Some(parent) = config.report.parent() {
        std::fs::create_dir_all(parent).expect("create explicit report directory");
    }
    {
        use std::io::Write;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config.report)
            .expect("create fresh soak report without replacing earlier evidence");
        output
            .write_all(&serde_json::to_vec_pretty(&report).expect("serialize soak report"))
            .expect("write soak report even on failed resource gate");
        output.sync_all().expect("flush numerical soak evidence");
    }
    assert!(result.is_ok(), "CPU lifecycle soak failed: {result:?}");
}
