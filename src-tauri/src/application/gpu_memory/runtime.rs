use super::{
    policy::{MemorySample, Observation},
    windows::Monitor,
    *,
};
use crate::{
    application::updates::activity,
    types::{AccelerationMode, GpuModelResidency},
};
use std::{sync::atomic::Ordering, time::Duration};

const POLL: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_secs(60);

struct Sampler {
    monitor: Option<Monitor>,
    retry_at: Instant,
    error: &'static str,
}

impl Default for Sampler {
    fn default() -> Self {
        Self {
            monitor: None,
            retry_at: Instant::now(),
            error: "Ожидаем измерение видеопамяти Windows.",
        }
    }
}

impl Sampler {
    fn sample(&mut self) -> Result<MemorySample, &'static str> {
        if self.monitor.is_none() {
            if Instant::now() < self.retry_at {
                return Err(self.error);
            }
            match Monitor::new() {
                Ok(monitor) => self.monitor = Some(monitor),
                Err(error) => {
                    self.error = error;
                    self.retry_at = Instant::now() + RETRY;
                    return Err(error);
                }
            }
        }
        let sample = self.monitor.as_mut().expect("initialized monitor").sample();
        match sample {
            Ok(sample) if sample.valid() => Ok(sample),
            _ => {
                self.error = sample
                    .err()
                    .unwrap_or("Windows вернула некорректный объём видеопамяти.");
                self.monitor = None;
                self.retry_at = Instant::now() + RETRY;
                Err(self.error)
            }
        }
    }
}

pub(super) fn start(app: AppHandle) {
    if app
        .state::<SttMemoryRuntime>()
        .started
        .swap(true, Ordering::AcqRel)
    {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut sampler = Sampler::default();
        loop {
            let tick_app = app.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                tick(&tick_app, &mut sampler);
                sampler
            })
            .await;
            sampler = result.unwrap_or_default();
            tokio::time::sleep(POLL).await;
        }
    });
}

fn tick(app: &AppHandle, sampler: &mut Sampler) {
    let state = app.state::<SttMemoryRuntime>();
    let settings = app.state::<AppState>().settings();
    let pipeline = app.state::<Pipeline>();
    let gpu = pipeline.stt().gpu_residency();
    let cpu = settings.acceleration == AccelerationMode::Cpu || (gpu.loaded && !gpu.resident);
    let adaptive = settings.gpu_model_residency == GpuModelResidency::Adaptive && !cpu;
    let sample = if adaptive {
        sampler.sample()
    } else {
        sampler.monitor = None;
        sampler.retry_at = Instant::now();
        Err("Адаптивный мониторинг выключен.")
    };
    let observation = Observation {
        mode: settings.gpu_model_residency,
        gpu_resident: gpu.resident,
        exclusive: gpu.exclusive,
        generation: gpu.generation,
        active: runtime_active(app) || activity::is_busy(),
        sample: sample.as_ref().ok().copied(),
    };
    let release = state
        .policy
        .lock()
        .observe(state.epoch.elapsed(), observation);
    let released = release && release_idle(app, observation.generation);
    if released {
        state.policy.lock().released();
    }
    let message = if released {
        "Модель освобождена из видеопамяти; загрузится при следующей диктовке."
    } else if sample.is_ok() && !gpu.resident {
        idle_message(
            cpu,
            false,
            settings.whisper_model_path.is_some(),
            &pipeline.stt().readiness(),
        )
    } else if sample.is_ok() {
        "Контролируем выделенную видеопамять Windows. При нехватке освободим модель между диктовками."
    } else {
        sample
            .as_ref()
            .err()
            .copied()
            .unwrap_or("Измерение недоступно.")
    };
    *state.snapshot.lock() = Snapshot {
        sample: sample.ok(),
        monitoring: if !adaptive {
            Monitoring::Disabled
        } else if observation.sample.is_some() {
            Monitoring::Enabled
        } else {
            Monitoring::Unsupported
        },
        message,
        observed: Instant::now(),
    };
}

fn release_idle(app: &AppHandle, expected_generation: u64) -> bool {
    let pipeline = app.state::<Pipeline>();
    let mut cleanup_lease = None;
    let released = pipeline
        .stt()
        .release_idle_gpu(expected_generation, |detach| {
            let admitted = activity::while_idle(
                || {
                    app.state::<AppState>().settings().gpu_model_residency
                        == GpuModelResidency::Adaptive
                        && !runtime_active(app)
                },
                detach,
            );
            match admitted {
                Some((released, lease)) => {
                    cleanup_lease = Some(lease);
                    released
                }
                None => false,
            }
        });
    // Worker destruction has finished before installer exclusion is released.
    drop(cleanup_lease);
    released
}
