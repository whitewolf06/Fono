//! Native GPU residency is independent of model installation/readiness.
mod policy;
#[cfg(windows)]
mod runtime;
#[cfg(windows)]
mod windows;

use crate::{pipeline::Pipeline, state::AppState, types::GpuModelResidency};
use parking_lot::Mutex;
use serde::Serialize;
use std::{sync::atomic::AtomicBool, time::Instant};
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Residency {
    Unloaded,
    Resident,
    Active,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Monitoring {
    Disabled,
    #[serde(rename = "monitoring")]
    Enabled,
    Unsupported,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SttMemoryStatus {
    mode: GpuModelResidency,
    residency: Residency,
    monitoring: Monitoring,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    used_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
}

struct Snapshot {
    sample: Option<policy::MemorySample>,
    monitoring: Monitoring,
    message: &'static str,
    observed: Instant,
}

pub(crate) struct SttMemoryRuntime {
    started: AtomicBool,
    snapshot: Mutex<Snapshot>,
    policy: Mutex<policy::Policy>,
    epoch: Instant,
}

impl Default for SttMemoryRuntime {
    fn default() -> Self {
        Self {
            started: AtomicBool::new(false),
            snapshot: Mutex::new(Snapshot {
                sample: None,
                monitoring: Monitoring::Unsupported,
                message: "Ожидаем измерение видеопамяти Windows.",
                observed: Instant::now(),
            }),
            policy: Mutex::new(policy::Policy::default()),
            epoch: Instant::now(),
        }
    }
}

pub(crate) fn start(app: AppHandle) {
    #[cfg(windows)]
    runtime::start(app);
    #[cfg(not(windows))]
    let _ = app;
}

pub(crate) fn status(app: &AppHandle) -> SttMemoryStatus {
    let settings = app.state::<AppState>().settings();
    let pipeline = app.state::<Pipeline>();
    let gpu = pipeline.stt().gpu_residency();
    let cpu = settings.acceleration == crate::types::AccelerationMode::Cpu
        || (gpu.loaded && !gpu.resident);
    let residency = if gpu.resident {
        if runtime_active(app) || !gpu.exclusive {
            Residency::Active
        } else {
            Residency::Resident
        }
    } else {
        Residency::Unloaded
    };
    let mut result = SttMemoryStatus {
        mode: settings.gpu_model_residency,
        residency,
        monitoring: Monitoring::Disabled,
        message: idle_message(
            cpu,
            gpu.resident,
            settings.whisper_model_path.is_some(),
            &pipeline.stt().readiness(),
        )
        .into(),
        used_bytes: None,
        total_bytes: None,
    };
    if settings.gpu_model_residency == GpuModelResidency::Adaptive && !cpu {
        #[cfg(windows)]
        {
            let state = app.state::<SttMemoryRuntime>();
            let snapshot = state.snapshot.lock();
            if snapshot.observed.elapsed().as_secs() > 15
                || matches!(snapshot.monitoring, Monitoring::Disabled)
            {
                result.monitoring = Monitoring::Unsupported;
                result.message = "Ожидаем актуальное измерение видеопамяти Windows.".into();
            } else {
                result.monitoring = snapshot.monitoring;
                result.message = snapshot.message.into();
                if let Some(sample) = snapshot.sample {
                    result.used_bytes = Some(sample.used_bytes);
                    result.total_bytes = Some(sample.total_bytes);
                }
            }
        }
        #[cfg(not(windows))]
        {
            result.monitoring = Monitoring::Unsupported;
            result.message = "Мониторинг видеопамяти поддерживается только в Windows.".into();
        }
    }
    result
}

fn idle_message(
    cpu: bool,
    gpu_resident: bool,
    model_selected: bool,
    readiness: &crate::stt::SttReadiness,
) -> &'static str {
    if !model_selected {
        return "Выберите модель распознавания в настройках.";
    }
    if matches!(readiness, crate::stt::SttReadiness::Loading) {
        return "Модель распознавания загружается.";
    }
    if matches!(readiness, crate::stt::SttReadiness::Failed { .. }) {
        return "Не удалось подготовить модель. Проверьте модель и ускорение в настройках распознавания.";
    }
    if gpu_resident {
        return "Модель остаётся в видеопамяти для быстрого старта.";
    }
    if cpu {
        return "Выбрано распознавание на CPU; видеопамять модели не занята.";
    }
    "Модель пока не находится в видеопамяти; загрузится по запросу."
}

fn runtime_active(app: &AppHandle) -> bool {
    let pipeline = app.state::<Pipeline>();
    if pipeline.current_operation().is_some()
        || pipeline.has_active_service_operation()
        || pipeline.scheduler().is_busy()
        || super::dictation::workflow::is_busy(app)
    {
        return true;
    }
    let queue = app
        .state::<super::service_control::ServiceControl>()
        .snapshot()
        .snapshot
        .queue;
    queue.queued + queue.preparing + queue.transcribing > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unloaded_and_unselected_models_do_not_claim_residency() {
        let empty = crate::stt::SttReadiness::Unloaded;
        assert!(idle_message(false, false, false, &empty).contains("Выберите"));
        assert!(idle_message(false, false, true, &empty).contains("пока не находится"));
        assert!(
            idle_message(false, false, true, &crate::stt::SttReadiness::Loading)
                .contains("загружается")
        );
    }

    #[test]
    fn failed_preparation_does_not_promise_a_ready_model_or_leak_details() {
        let failed = crate::stt::SttReadiness::Failed {
            message: "private path".into(),
        };
        let message = idle_message(false, true, true, &failed);
        assert!(message.contains("Не удалось подготовить"));
        assert!(!message.contains("private path"));
    }
}
