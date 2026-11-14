//! Device-scoped admission for server inference windows. The desktop's
//! `SttScheduler` keeps its capture/Stop reservation rules unchanged.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use fono_core::OperationCancellation;
use parking_lot::{Condvar, Mutex};

use crate::error::{SttError, SttResult};

/// Physical placement resolved by the caller before entering the queue.
/// `AccelerationMode::Auto` is not a device identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceId {
    Cpu,
    Cuda(u32),
    Vulkan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueClass {
    Interactive,
    Batch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceQueueStatus {
    pub capacity: usize,
    pub active_interactive: usize,
    pub active_batch: usize,
    pub queued_interactive: usize,
    pub queued_batch: usize,
}

struct ActiveWindow {
    class: QueueClass,
    cancellation: OperationCancellation,
}

struct DeviceLane {
    capacity: usize,
    interactive: VecDeque<u64>,
    batch: VecDeque<u64>,
    active: HashMap<u64, ActiveWindow>,
}

impl DeviceLane {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            interactive: VecDeque::new(),
            batch: VecDeque::new(),
            active: HashMap::new(),
        }
    }

    fn queue_mut(&mut self, class: QueueClass) -> &mut VecDeque<u64> {
        match class {
            QueueClass::Interactive => &mut self.interactive,
            QueueClass::Batch => &mut self.batch,
        }
    }

    fn can_start(&self, ticket: u64, class: QueueClass) -> bool {
        if self.active.len() >= self.capacity {
            return false;
        }
        match class {
            QueueClass::Interactive => self.interactive.front() == Some(&ticket),
            QueueClass::Batch => self.interactive.is_empty() && self.batch.front() == Some(&ticket),
        }
    }

    fn status(&self) -> DeviceQueueStatus {
        DeviceQueueStatus {
            capacity: self.capacity,
            active_interactive: self
                .active
                .values()
                .filter(|window| window.class == QueueClass::Interactive)
                .count(),
            active_batch: self
                .active
                .values()
                .filter(|window| window.class == QueueClass::Batch)
                .count(),
            queued_interactive: self.interactive.len(),
            queued_batch: self.batch.len(),
        }
    }
}

struct State {
    next_ticket: u64,
    lanes: HashMap<DeviceId, DeviceLane>,
}

/// Each physical device has one priority queue. CPU parallelism shares that
/// queue; each CUDA device has exactly one slot. The caller acquires a permit
/// per bounded inference window and keeps it until native work has stopped.
pub struct DeviceScheduler {
    state: Mutex<State>,
    changed: Condvar,
}

/// An active inference window. Dropping the permit releases its device slot;
/// dropping an IPC waiter must not drop a permit still owned by native work.
#[must_use = "keep the permit until inference and native cleanup finish"]
pub struct DevicePermit {
    scheduler: Arc<DeviceScheduler>,
    device: DeviceId,
    ticket: u64,
    pub cancellation: OperationCancellation,
}

impl DeviceScheduler {
    /// `cpu_parallel` must be positive. GPU devices are admitted one at a
    /// time; duplicates in `cuda_devices` do not create extra capacity.
    pub fn new(
        cpu_parallel: usize,
        cuda_devices: impl IntoIterator<Item = u32>,
    ) -> SttResult<Self> {
        if cpu_parallel == 0 {
            return Err(SttError::Config("cpu_parallel must be positive".into()));
        }
        let mut lanes = HashMap::from([(DeviceId::Cpu, DeviceLane::new(cpu_parallel))]);
        for index in cuda_devices {
            lanes
                .entry(DeviceId::Cuda(index))
                .or_insert_with(|| DeviceLane::new(1));
        }
        Ok(Self {
            state: Mutex::new(State {
                next_ticket: 0,
                lanes,
            }),
            changed: Condvar::new(),
        })
    }

    /// One physical Vulkan adapter selected by the worker. Its own lane keeps
    /// interactive/API inference within the device's admission capacity.
    pub fn with_vulkan_device(mut self) -> Self {
        self.state
            .get_mut()
            .lanes
            .insert(DeviceId::Vulkan, DeviceLane::new(1));
        self
    }

    pub fn status(&self, device: DeviceId) -> Option<DeviceQueueStatus> {
        self.state.lock().lanes.get(&device).map(DeviceLane::status)
    }

    /// Interactive waiters run before queued batch windows on the same
    /// device. They cancel active batch *window* tokens, leaving the batch job
    /// token intact for a retry from its last checkpoint.
    pub fn acquire(
        self: &Arc<Self>,
        device: DeviceId,
        class: QueueClass,
        cancel: &OperationCancellation,
    ) -> SttResult<DevicePermit> {
        if cancel.is_cancelled() {
            return Err(SttError::Cancelled("STT request cancelled".into()));
        }
        let mut state = self.state.lock();
        if !state.lanes.contains_key(&device) {
            return Err(SttError::Config(format!(
                "STT device {device:?} is not configured"
            )));
        }
        let ticket = state
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| SttError::Internal("STT queue ticket exhausted".into()))?;
        state.next_ticket = ticket;
        let lane = state.lanes.get_mut(&device).expect("device checked above");
        lane.queue_mut(class).push_back(ticket);
        if class == QueueClass::Interactive {
            let free = lane.capacity.saturating_sub(lane.active.len());
            let yielding = lane
                .active
                .values()
                .filter(|window| {
                    window.class == QueueClass::Batch && window.cancellation.is_cancelled()
                })
                .count();
            let mut needed = lane
                .interactive
                .len()
                .saturating_sub(free.saturating_add(yielding));
            for window in lane.active.values() {
                if needed == 0 {
                    break;
                }
                if window.class == QueueClass::Batch && !window.cancellation.is_cancelled() {
                    window.cancellation.cancel();
                    needed -= 1;
                }
            }
        }
        self.changed.notify_all();

        loop {
            let lane = state
                .lanes
                .get_mut(&device)
                .expect("configured device persists");
            if cancel.is_cancelled() {
                lane.queue_mut(class).retain(|queued| *queued != ticket);
                self.changed.notify_all();
                return Err(SttError::Cancelled("STT queue wait cancelled".into()));
            }
            if lane.can_start(ticket, class) {
                let front = lane.queue_mut(class).pop_front();
                debug_assert_eq!(front, Some(ticket));
                let cancellation = OperationCancellation::default().combined_with(cancel);
                lane.active.insert(
                    ticket,
                    ActiveWindow {
                        class,
                        cancellation: cancellation.clone(),
                    },
                );
                self.changed.notify_all();
                return Ok(DevicePermit {
                    scheduler: Arc::clone(self),
                    device,
                    ticket,
                    cancellation,
                });
            }
            self.changed.wait_for(&mut state, Duration::from_millis(20));
        }
    }
}

impl Drop for DevicePermit {
    fn drop(&mut self) {
        let mut state = self.scheduler.state.lock();
        if let Some(lane) = state.lanes.get_mut(&self.device) {
            lane.active.remove(&self.ticket);
        }
        self.scheduler.changed.notify_all();
    }
}
