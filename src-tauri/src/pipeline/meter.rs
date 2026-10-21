//! Numeric UI telemetry is sampled outside the audio callback, only in capture.
use super::*;
use std::time::{Duration, Instant};

pub(super) fn sync(app: &AppHandle, state: PipelineState) {
    let pipeline = app.state::<Pipeline>();
    if state != PipelineState::Listening {
        pipeline.meter_operation.store(0, Ordering::Release);
        return;
    }
    let operation = pipeline.operation_id();
    if operation == 0 || pipeline.meter_operation.swap(operation, Ordering::AcqRel) == operation {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let pipeline = app.state::<Pipeline>();
            if pipeline.meter_operation.load(Ordering::Acquire) != operation
                || pipeline.operation_id() != operation
            {
                break;
            }
            crate::events::emit_audio_level(
                &app,
                crate::events::AudioLevelEventV1 {
                    operation_id: operation,
                    level: pipeline.current_level().clamp(0.0, 1.0),
                    elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                    pretranscription: pipeline.pretranscription_status_for(operation),
                    auto_stop: pipeline.auto_stop_status_for(operation),
                },
            );
        }
    });
}
