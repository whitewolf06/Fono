//! Conservative pressure policy. Durations use monotonic time; no process
//! names, activity guesses or user content participate in the decision.
use crate::types::GpuModelResidency;
use std::time::Duration;

pub(super) const IDLE_HOLD: Duration = Duration::from_secs(60);
pub(super) const PRESSURE_HOLD: Duration = Duration::from_secs(20);
pub(super) const LOAD_HOLD: Duration = Duration::from_secs(180);
const MIN_CAPACITY: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MemorySample {
    pub used_bytes: u64,
    pub total_bytes: u64,
}

impl MemorySample {
    pub fn valid(self) -> bool {
        self.total_bytes >= MIN_CAPACITY && self.used_bytes <= self.total_bytes
    }

    fn pressure(self) -> bool {
        let free = self.total_bytes - self.used_bytes;
        // Require both high occupancy and little headroom. This avoids
        // unloading a large-card model merely because its percentage is high.
        self.used_bytes as f64 / self.total_bytes as f64 >= 0.90 && free <= 1024 * 1024 * 1024
    }

    fn recovered(self) -> bool {
        self.used_bytes as f64 / self.total_bytes as f64 <= 0.82
            || self.total_bytes - self.used_bytes >= 1536 * 1024 * 1024
    }
}

#[derive(Clone, Copy)]
pub(super) struct Observation {
    pub mode: GpuModelResidency,
    pub gpu_resident: bool,
    pub exclusive: bool,
    pub active: bool,
    pub generation: u64,
    pub sample: Option<MemorySample>,
}

#[derive(Default)]
pub(super) struct Policy {
    generation: Option<u64>,
    loaded_at: Option<Duration>,
    idle_since: Option<Duration>,
    pressure_since: Option<Duration>,
}

fn held(now: Duration, since: Option<Duration>, required: Duration) -> bool {
    since.is_some_and(|since| now.saturating_sub(since) >= required)
}

impl Policy {
    pub fn observe(&mut self, now: Duration, observation: Observation) -> bool {
        if self.generation != Some(observation.generation) {
            self.generation = Some(observation.generation);
            self.loaded_at = Some(now);
            self.idle_since = None;
            self.pressure_since = None;
        }
        if observation.mode != GpuModelResidency::Adaptive || !observation.gpu_resident {
            self.idle_since = None;
            self.pressure_since = None;
            return false;
        }
        if observation.active || !observation.exclusive {
            self.idle_since = None;
        } else {
            self.idle_since.get_or_insert(now);
        }
        let Some(sample) = observation.sample.filter(|sample| sample.valid()) else {
            // A missing or invalid measurement breaks sustained pressure.
            self.pressure_since = None;
            return false;
        };
        if sample.pressure() {
            self.pressure_since.get_or_insert(now);
        } else if sample.recovered() {
            self.pressure_since = None;
        }
        held(now, self.loaded_at, LOAD_HOLD)
            && held(now, self.idle_since, IDLE_HOLD)
            && held(now, self.pressure_since, PRESSURE_HOLD)
    }

    pub fn released(&mut self) {
        self.idle_since = None;
        self.pressure_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const GIB: u64 = 1024 * 1024 * 1024;

    fn observation() -> Observation {
        Observation {
            mode: GpuModelResidency::Adaptive,
            gpu_resident: true,
            exclusive: true,
            active: false,
            generation: 1,
            sample: Some(MemorySample {
                used_bytes: 15 * GIB,
                total_bytes: 16 * GIB,
            }),
        }
    }

    #[test]
    fn sustained_pressure_waits_for_idle_and_reload_hold() {
        let mut policy = Policy::default();
        assert!(!policy.observe(Duration::ZERO, observation()));
        assert!(!policy.observe(LOAD_HOLD - Duration::from_secs(1), observation()));
        assert!(policy.observe(LOAD_HOLD, observation()));
        policy.released();
        let mut next = observation();
        next.generation = 2;
        assert!(!policy.observe(LOAD_HOLD, next));
        assert!(!policy.observe(LOAD_HOLD + IDLE_HOLD, next));
        assert!(policy.observe(LOAD_HOLD + LOAD_HOLD, next));
    }

    #[test]
    fn active_operations_and_retained_handles_restart_idle_hold() {
        for retained_handle in [false, true] {
            let mut policy = Policy::default();
            policy.observe(Duration::ZERO, observation());
            let mut busy = observation();
            busy.active = !retained_handle;
            busy.exclusive = !retained_handle;
            assert!(!policy.observe(LOAD_HOLD, busy));
            assert!(!policy.observe(LOAD_HOLD, observation()));
            assert!(!policy.observe(
                LOAD_HOLD + IDLE_HOLD - Duration::from_secs(1),
                observation()
            ));
            assert!(policy.observe(LOAD_HOLD + IDLE_HOLD, observation()));
        }
    }

    #[test]
    fn resident_cpu_missing_and_invalid_samples_never_release() {
        let mut variants = Vec::new();
        let mut resident = observation();
        resident.mode = GpuModelResidency::Resident;
        variants.push(resident);
        let mut cpu = observation();
        cpu.gpu_resident = false;
        variants.push(cpu);
        let mut unknown = observation();
        unknown.sample = None;
        variants.push(unknown);
        for sample in [
            MemorySample {
                used_bytes: 2,
                total_bytes: 1,
            },
            MemorySample {
                used_bytes: 0,
                total_bytes: 0,
            },
        ] {
            let mut invalid = observation();
            invalid.sample = Some(sample);
            variants.push(invalid);
        }
        for variant in variants {
            let mut policy = Policy::default();
            assert!(!policy.observe(Duration::ZERO, variant));
            assert!(!policy.observe(LOAD_HOLD * 2, variant));
        }
    }

    #[test]
    fn recovery_or_missing_sample_breaks_pressure_and_prevents_churn() {
        for sample in [
            None,
            Some(MemorySample {
                used_bytes: 12 * GIB,
                total_bytes: 16 * GIB,
            }),
        ] {
            let mut policy = Policy::default();
            policy.observe(Duration::ZERO, observation());
            let mut recovered = observation();
            recovered.sample = sample;
            assert!(!policy.observe(LOAD_HOLD, recovered));
            assert!(!policy.observe(LOAD_HOLD, observation()));
            assert!(policy.observe(LOAD_HOLD + PRESSURE_HOLD, observation()));
        }
    }

    #[test]
    fn large_cards_with_headroom_and_clock_rollback_do_not_release() {
        let mut policy = Policy::default();
        let mut headroom = observation();
        headroom.sample = Some(MemorySample {
            used_bytes: 30 * GIB,
            total_bytes: 32 * GIB,
        });
        assert!(!policy.observe(Duration::from_secs(500), headroom));
        assert!(!policy.observe(Duration::from_secs(1000), headroom));
        assert!(!policy.observe(Duration::ZERO, observation()));
    }
}
