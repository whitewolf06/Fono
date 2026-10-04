//! GPU detachment is separate from model selection and public readiness.
use super::*;

#[derive(Clone, Copy)]
pub(crate) struct GpuResidency {
    pub loaded: bool,
    pub resident: bool,
    pub exclusive: bool,
    pub generation: u64,
}

fn exclusive<T>(handle: &Arc<T>) -> bool {
    Arc::strong_count(handle) == 1
}

fn resident_gpu(state: &EngineState) -> (bool, bool) {
    match state {
        EngineState::Empty => (false, false),
        EngineState::Worker { session, device } => {
            let gpu = matches!(*device, "CUDA" | "Vulkan");
            (gpu, gpu && exclusive(session) && session.is_idle())
        }
        EngineState::Embedded { engine, device } => {
            let gpu = matches!(*device, "CUDA" | "Vulkan");
            (gpu, gpu && exclusive(engine) && engine.try_lock().is_some())
        }
    }
}

impl SttEngine {
    pub(crate) fn gpu_residency(&self) -> GpuResidency {
        let state = self.state.lock();
        let (resident, exclusive) = resident_gpu(&state);
        GpuResidency {
            loaded: !matches!(*state, EngineState::Empty),
            resident,
            exclusive,
            generation: self.generation.load(std::sync::atomic::Ordering::Acquire),
        }
    }

    /// The caller fences operations, queues and leases around `detach`. Worker
    /// cleanup runs outside that admission fence, but inside the load gate, so
    /// a new on-demand load never competes with VRAM that is still being freed.
    pub(crate) fn release_idle_gpu(
        &self,
        expected_generation: u64,
        fenced: impl FnOnce(&mut dyn FnMut() -> bool) -> bool,
    ) -> bool {
        let Some(_load) = self.load_gate.try_lock() else {
            return false;
        };
        {
            let _state = self.state.lock();
            // Sampling and the pressure hold belong to this exact load. A
            // replacement may have finished between observation and release.
            // The load gate keeps generation stable until detachment finishes.
            if self.generation.load(std::sync::atomic::Ordering::Acquire) != expected_generation {
                return false;
            }
        }
        let mut previous = None;
        let released = fenced(&mut || {
            let mut state = self.state.lock();
            if !resident_gpu(&state).1 {
                return false;
            }
            previous = Some(std::mem::replace(&mut *state, EngineState::Empty));
            true
        });
        drop(previous);
        // Keep readiness Ready: the selected model remains installed and can
        // load on the next request. Memory status reports actual residency.
        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_operation_handles_prevent_detachment() {
        let handle = Arc::new(());
        assert!(exclusive(&handle));
        let active = handle.clone();
        assert!(!exclusive(&handle));
        drop(active);
        assert!(exclusive(&handle));
    }

    #[test]
    fn an_empty_engine_never_claims_gpu_memory() {
        let engine = SttEngine::new();
        let residency = engine.gpu_residency();
        assert!(!residency.resident);
        assert!(!residency.exclusive);
        assert!(!engine.release_idle_gpu(residency.generation, |detach| detach()));
    }

    #[test]
    fn stale_pressure_cannot_admit_release_of_a_fresh_generation() {
        let engine = SttEngine::new();
        let observed = engine.gpu_residency().generation;
        engine
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        assert!(!engine.release_idle_gpu(observed, |_| {
            panic!("stale pressure reached the admission fence")
        }));
        let mut reached_fence = false;
        assert!(!engine.release_idle_gpu(observed + 1, |detach| {
            reached_fence = true;
            detach()
        }));
        assert!(reached_fence);
    }
}
