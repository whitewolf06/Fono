use std::sync::Arc;

use parking_lot::Mutex;

use crate::event::WakeWordEvent;

pub type WakeCallback = Arc<dyn Fn(WakeWordEvent) + Send + Sync>;

#[derive(Clone, Default)]
pub(crate) struct CallbackSlot {
    inner: Arc<Mutex<Option<WakeCallback>>>,
}

impl CallbackSlot {
    pub(crate) fn set(&self, callback: WakeCallback) {
        *self.inner.lock() = Some(callback);
    }

    pub(crate) fn notify(&self, event: WakeWordEvent) {
        let callback = self.inner.lock().clone();
        if let Some(callback) = callback {
            callback(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::CallbackSlot;
    use crate::event::WakeWordEvent;

    #[test]
    fn callback_is_invoked_without_holding_the_slot_lock() {
        let slot = CallbackSlot::default();
        let slot_from_callback = slot.clone();
        let replaced = std::sync::Arc::new(AtomicBool::new(false));
        let replaced_from_callback = replaced.clone();

        slot.set(std::sync::Arc::new(move |_| {
            replaced_from_callback.store(true, Ordering::SeqCst);
            slot_from_callback.set(std::sync::Arc::new(|_| {}));
        }));

        slot.notify(WakeWordEvent::Listening);
        assert!(replaced.load(Ordering::SeqCst));
    }
}
