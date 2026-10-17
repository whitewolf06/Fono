/// ACK acknowledges the request, not process exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkerCleanupStatus {
    Exited,
    Pending,
}

pub(super) fn worker_cleanup_busy() -> AppError {
    AppError::Busy("STT worker cleanup is still pending; retry after it exits".into())
}

pub(super) struct WorkerChild {
    process: Mutex<Child>,
    // File and directory sharing guards outlive every process/IO owner,
    // including deferred cleanup after a failed handshake or load.
    _package: Option<crate::application::cuda_packages::VerifiedPackage>,
    stopping: AtomicBool,
    #[cfg(test)]
    kill_count: AtomicUsize,
    #[cfg(test)]
    force_kill_fail: AtomicBool,
}

impl WorkerChild {
    fn new(child: Child, package: Option<crate::application::cuda_packages::VerifiedPackage>) -> Self {
        Self {
            process: Mutex::new(child),
            _package: package,
            stopping: AtomicBool::new(false),
            #[cfg(test)]
            kill_count: AtomicUsize::new(0),
            #[cfg(test)]
            force_kill_fail: AtomicBool::new(false),
        }
    }

    fn try_wait(&self) -> std::io::Result<Option<ExitStatus>> {
        self.process.lock().try_wait()
    }

    fn kill(&self) -> std::io::Result<()> {
        #[cfg(test)]
        {
            self.kill_count.fetch_add(1, Ordering::AcqRel);
            if self.force_kill_fail.load(Ordering::Acquire) {
                return Err(std::io::Error::from(ErrorKind::PermissionDenied));
            }
        }
        self.process.lock().kill()
    }

    fn exited(&self) -> bool {
        matches!(self.try_wait(), Ok(Some(_)))
    }

    fn await_exit(&self, deadline: Instant) -> bool {
        loop {
            if self.exited() {
                return true;
            }
            let remaining = remaining_until(deadline);
            if remaining.is_zero() {
                return false;
            }
            thread::sleep(remaining.min(CANCELLATION_POLL_INTERVAL));
        }
    }

    fn force_exit(&self, deadline: Instant) -> bool {
        if self.exited() {
            return true;
        }
        let _ = self.kill();
        self.await_exit(deadline)
    }
}

/// Retirements are scoped to one STT engine and survive failed starts as well.
#[derive(Default)]
pub(super) struct WorkerCleanupSupervisor {
    pending: AtomicUsize,
}

struct WorkerCleanupFence(Arc<WorkerCleanupSupervisor>);
impl Drop for WorkerCleanupFence {
    fn drop(&mut self) {
        self.0.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

impl WorkerCleanupSupervisor {
    fn begin(self: &Arc<Self>) -> WorkerCleanupFence {
        self.pending.fetch_add(1, Ordering::AcqRel);
        WorkerCleanupFence(Arc::clone(self))
    }
    pub(super) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) != 0
    }

    fn retire(
        self: &Arc<Self>,
        child: Arc<WorkerChild>,
        handles: Vec<JoinHandle<()>>,
        stopping: Arc<AtomicBool>,
        activity: Option<crate::application::updates::activity::ActivityLease<'static>>,
    ) -> Arc<AtomicBool> {
        struct Retirement {
            child: Arc<WorkerChild>,
            handles: Vec<JoinHandle<()>>,
            stopping: Arc<AtomicBool>,
            _activity: Option<crate::application::updates::activity::ActivityLease<'static>>,
        }
        self.pending.fetch_add(1, Ordering::AcqRel);
        let completed = Arc::new(AtomicBool::new(false));
        let owned = Arc::new(Mutex::new(Some(Retirement {
            child,
            handles,
            stopping,
            _activity: activity,
        })));
        let owner = Arc::clone(&owned);
        let supervisor = Arc::clone(self);
        let completion = Arc::clone(&completed);
        let spawned = thread::Builder::new()
            .name("fono-worker-cleanup".into())
            .spawn(move || {
                let Some(mut retirement) = owner.lock().take() else {
                    return;
                };
                retirement.stopping.store(true, Ordering::Release);
                loop {
                    if !retirement.child.exited() {
                        let _ = retirement.child.kill();
                    }
                    cancel_thread_io(&retirement.handles);
                    retirement.handles = join_finished(retirement.handles);
                    if retirement.child.exited() && retirement.handles.is_empty() {
                        break;
                    }
                    thread::sleep(CANCELLATION_POLL_INTERVAL);
                }
                // Admission is released only after confirmed exit and finished IO,
                // and outside every process/supervisor mutex.
                drop(retirement);
                completion.store(true, Ordering::Release);
                supervisor.pending.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            // Retain ownership and fail closed if OS thread creation fails.
            std::mem::forget(owned);
            tracing::error!("worker cleanup remains fenced: supervisor thread unavailable");
        }
        completed
    }
}

fn cancel_thread_io(handles: &[JoinHandle<()>]) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn CancelSynchronousIo(thread: *mut std::ffi::c_void) -> i32;
        }
        for handle in handles {
            if !handle.is_finished() {
                // Handles belong only to this worker's managed threads.
                unsafe {
                    CancelSynchronousIo(handle.as_raw_handle());
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = handles;
}

fn join_finished(handles: Vec<JoinHandle<()>>) -> Vec<JoinHandle<()>> {
    handles
        .into_iter()
        .filter_map(|handle| {
            if handle.is_finished() {
                let _ = handle.join();
                None
            } else {
                Some(handle)
            }
        })
        .collect()
}

fn bounded_join(mut handles: Vec<JoinHandle<()>>, deadline: Instant) -> Vec<JoinHandle<()>> {
    loop {
        cancel_thread_io(&handles);
        handles = join_finished(handles);
        if handles.is_empty() || Instant::now() >= deadline {
            return handles;
        }
        thread::sleep(CANCELLATION_POLL_INTERVAL);
    }
}

fn terminate_unstarted_child(
    child: Child,
    cleanup: &Arc<WorkerCleanupSupervisor>,
    package: Option<crate::application::cuda_packages::VerifiedPackage>,
) {
    let activity = Some(crate::application::updates::activity::cleanup_lease());
    let _fence = cleanup.begin();
    let child = Arc::new(WorkerChild::new(child, package));
    if !child.force_exit(Instant::now() + FORCE_EXIT_TIMEOUT) {
        cleanup.retire(child, Vec::new(), Arc::new(AtomicBool::new(true)), activity);
    }
}
