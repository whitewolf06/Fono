# fono-stt

Shared recognition engine, scheduler, segmentation and model services for Fono.
The crate has no Tauri or desktop dependency. Fono Desktop composes it through
`src-tauri/src/stt` and the model-service adapters in `src-tauri/src/application`.

`SttEngine::new(SttEnvironment)` receives model/resource paths, worker paths,
GPU packages, activity admission, readiness observation and safe error messages.
`ensure_model_loaded` uses that environment; the explicit-path loading methods
remain available for desktop selection snapshots and recovery. Recognition keeps
the existing cancellation, scheduler priority, worker protocol and timestamps.

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
