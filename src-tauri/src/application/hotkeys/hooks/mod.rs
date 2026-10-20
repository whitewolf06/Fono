//! Bounded button queue and its worker; native callbacks never run dictation.
#[cfg(windows)]
mod windows;

use super::{
    gesture::{Edge, Gesture, GestureState},
    state::ShortcutState,
};
use crate::types::{DictationTrigger, HotkeyMode};
use crossbeam_queue::ArrayQueue;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub(crate) struct HookRuntime(Mutex<Option<Session>>);

impl HookRuntime {
    pub fn stop(&self) {
        self.0.lock().take(); // Drop joins both threads before replacement/exit.
    }

    pub fn start(
        &self,
        app: &AppHandle,
        trigger: DictationTrigger,
        mode: HotkeyMode,
    ) -> Result<(), &'static str> {
        if trigger == DictationTrigger::Shortcut {
            return Ok(());
        }
        #[cfg(windows)]
        {
            *self.0.lock() = Some(Session::start(app.clone(), trigger, mode)?);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = (app, mode);
            Err("Триггеры клавиатуры и мыши доступны только в Windows.")
        }
    }
}

struct Queue {
    edges: ArrayQueue<Edge>,
    enabled: AtomicBool,
    fault: AtomicBool,
    stopping: AtomicBool,
}

impl Queue {
    fn new(capacity: usize) -> Self {
        Self {
            edges: ArrayQueue::new(capacity),
            enabled: AtomicBool::new(true),
            fault: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        }
    }

    /// Nonblocking, allocation-free; overflow disables further dispatch.
    fn push(&self, edge: Edge) {
        if self.enabled.load(Ordering::Acquire) && self.edges.push(edge).is_err() {
            self.fail();
        }
    }

    fn fail(&self) {
        self.enabled.store(false, Ordering::Release);
        self.fault.store(true, Ordering::Release);
    }
}

struct Session {
    queue: Arc<Queue>,
    worker: Option<JoinHandle<()>>,
    #[cfg(windows)]
    hooks: Option<windows::HookThread>,
}

impl Session {
    #[cfg(windows)]
    fn start(
        app: AppHandle,
        trigger: DictationTrigger,
        mode: HotkeyMode,
    ) -> Result<Self, &'static str> {
        let queue = Arc::new(Queue::new(64));
        let worker_queue = queue.clone();
        let worker = thread::Builder::new()
            .name("fono-trigger-actions".into())
            .spawn(move || run(app, worker_queue, trigger, mode))
            .map_err(|_| {
                "Не удалось запустить обработку триггера. Выберите обычное сочетание клавиш."
            })?;
        let mut session = Self {
            queue: queue.clone(),
            worker: Some(worker),
            hooks: None,
        };
        session.hooks = Some(windows::HookThread::start(queue, trigger)?);
        Ok(session)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.queue.enabled.store(false, Ordering::Release);
        self.queue.stopping.store(true, Ordering::Release);
        #[cfg(windows)]
        self.hooks.take(); // Unhook on the message-loop thread, then join it.
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

fn run(app: AppHandle, queue: Arc<Queue>, trigger: DictationTrigger, mode: HotkeyMode) {
    let state = Arc::new(Mutex::new(ShortcutState::default()));
    let mut gesture = GestureState::new(trigger, mode);
    while !queue.stopping.load(Ordering::Acquire) {
        if queue.fault.load(Ordering::Acquire) {
            // Losing an edge can lose a hold Release: cancel only our operation,
            // never finish/insert it or touch a later UI/command recording.
            let operation = state.lock().owned_recording(super::active_capture(&app));
            if let Some(operation) = operation {
                if let Ok(activity) = crate::application::updates::activity::lease() {
                    let cancel_app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _activity = activity;
                        let _ =
                            crate::ipc::dictation::cancel_dictation(cancel_app, Some(operation))
                                .await;
                    });
                }
            }
            crate::events::emit_error(&app, crate::events::ErrorCodeV1::Internal,
                "Триггер отключён после потери событий ввода. Выберите его заново или используйте обычное сочетание клавиш.", operation);
            return;
        }
        let Some(edge) = queue.edges.pop() else {
            thread::park_timeout(Duration::from_millis(5));
            continue;
        };
        // A saved hook may be installed during setup, before history/services
        // exist. Unlike RegisterHotKey this worker already has a running loop.
        if !queue.enabled.load(Ordering::Acquire)
            || app
                .try_state::<crate::app_runtime::RuntimeReady>()
                .is_none()
        {
            continue;
        }
        match gesture.event(edge) {
            Gesture::None => {}
            Gesture::Edge(event) => super::handle(&app, &state, event, mode, false),
            Gesture::Toggle => {
                super::handle(
                    &app,
                    &state,
                    super::state::KeyEvent::Pressed,
                    HotkeyMode::Toggle,
                    false,
                );
                super::handle(
                    &app,
                    &state,
                    super::state::KeyEvent::Released,
                    HotkeyMode::Toggle,
                    false,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_disables_dispatch_and_cannot_lose_release_silently() {
        let queue = Queue::new(1);
        queue.push(Edge {
            pressed: true,
            time: 1,
        });
        queue.push(Edge {
            pressed: false,
            time: 2,
        });
        assert!(!queue.enabled.load(Ordering::Acquire));
        assert!(queue.fault.load(Ordering::Acquire));
        assert_eq!(queue.edges.len(), 1);
        queue.edges.pop();
        queue.push(Edge {
            pressed: true,
            time: 3,
        });
        assert!(queue.edges.is_empty());
    }

    #[test]
    fn dropping_runtime_stops_and_joins_action_worker_before_returning() {
        let queue = Arc::new(Queue::new(2));
        let worker_queue = queue.clone();
        let exited = Arc::new(AtomicBool::new(false));
        let worker_exited = exited.clone();
        let worker = thread::spawn(move || {
            while !worker_queue.stopping.load(Ordering::Acquire) {
                thread::park();
            }
            worker_exited.store(true, Ordering::Release);
        });
        let runtime = HookRuntime(Mutex::new(Some(Session {
            queue: queue.clone(),
            worker: Some(worker),
            hooks: None,
        })));
        runtime.stop();
        assert!(!queue.enabled.load(Ordering::Acquire));
        assert!(exited.load(Ordering::Acquire));
        runtime.stop(); // Partial setup/repeated shutdown is harmless.
    }
}
