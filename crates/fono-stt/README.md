# fono-stt

Shared recognition engine, scheduler, segmentation and model services for Fono.
The crate has no Tauri or desktop dependency. Fono Desktop composes it through
`src-tauri/src/stt` and the model-service adapters in `src-tauri/src/application`.

`SttEngine::new(SttEnvironment)` receives model/resource paths, worker paths,
GPU packages, activity admission, readiness observation and safe error messages.
`ensure_model_loaded` uses that environment; the explicit-path loading methods
remain available for desktop selection snapshots and recovery. Recognition keeps
the existing cancellation, scheduler priority, worker protocol and timestamps.

`ModelPool` provides independent engines for server deployments while desktop
keeps its single engine. Its hard `max_loaded_models` limit counts loading and
retiring slots, including worker cleanup still in progress. `acquire` pins a
model through a non-cloneable `ModelLease`; capacity pressure evicts the least
recently used inactive model, and `reap_idle` retires expired `IdleUnload`
models on the host's maintenance tick. `Resident` suppresses time-based
unloading but remains eligible for inactive LRU eviction under the hard limit.
The optional byte cap uses estimates supplied by the host for admission, not
measured RAM or VRAM. Keep the lease inside the actual background work so an
abandoned IPC waiter cannot free a slot while recognition is still running.
When every reservation is active or retiring, `acquire` returns `SttError::Busy`;
the host may retry after work or cleanup finishes.
Each `ModelId` is bound to one path and placement for the pool's lifetime.
Current custom Whisper paths all share the `custom` ID, so a single pool
rejects a second custom file instead of silently switching the first one.

`scheduler::DeviceScheduler` admits server inference per resolved physical
device (`Cpu` or `Cuda(index)`). CPU has a configurable shared slot limit;
each configured CUDA device has one slot. Each device has FIFO queues for
interactive and batch windows, with interactive waiters admitted first. When
the device is full, an interactive waiter cancels active batch *window* tokens;
the batch job keeps its own cancellation token and retries from a checkpoint.
The host must acquire a new permit for each bounded batch window and retain
that permit through the end of native inference and cleanup. Queue counts are
observable through `status`; server queue size and token quotas are separate
host policies. Desktop continues to use `SttScheduler` and its existing
capture/Stop reservation behavior.

Host implementations must retain ownership through actual background completion:

- `ActivityGuard::lease` admits work; `cleanup_lease` retains already admitted
  teardown even when new work is blocked. An owning `ActivityLease` is shared
  through pending commands, recovery and deferred worker cleanup.
- `GpuPackageHandle` owns the provider's verification/file guards. Its identity
  and worker path identify a recovery snapshot. Final layout validation happens
  before spawn; guards outlive the worker and its outstanding I/O threads.
- `DownloadRegistration` holds cancellation and admission for the actual blocking
  download. Desktop model and legacy wake downloads use the same registry.

`SttError` preserves backend metadata for host adapters. `CoreError::from(error)`
provides stable transport codes and authored messages without diagnostic source
strings. Desktop retains its protected IPC error shape and localization catalog
through its own adapter and the injected message callback.

Model services accept explicit directories, cancellation and `DownloadProgress`.
They retain checksum verification and atomic publication. Window/menu events,
settings, capture configuration, installation admission and async IPC ownership
stay in the desktop adapters.

Default features enable speech VAD and offline STT. CUDA, Vulkan and the CPU-only
profile are explicit features forwarded by the host. `test-support` exposes only
the owned worker controls needed by the desktop's opt-in subsystem soak; it is
enabled by the desktop dev dependency, not its production dependency.

Tests for the moved algorithms live with the crate. Physical microphone,
hotkey/input delivery, GPU driver behavior and installed-application acceptance
are separate owner checks. Windows-specific desktop soak scenarios remain in
the desktop crate.
