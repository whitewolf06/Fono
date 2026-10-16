//! Detect a second launch before services/settings are initialized.
//!
//! Register `plugin()` first. Hold `startup_gate()` across `Builder::build`, then
//! release it before `App::run`: the released Windows plugin otherwise has a race
//! between publishing its named mutex and creating its message target window.

use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;

use sha2::{Digest, Sha256};
use tauri::{plugin::TauriPlugin, Manager, Runtime};

/// The Windows plugin keys its mutex from the application identifier, not dbus_id.
/// Debug review profiles must not activate or stop the installed application.
pub(crate) fn context<R: Runtime>(
    mut context: tauri::Context<R>,
) -> Result<tauri::Context<R>, &'static str> {
    let test_dir = std::env::var_os("FONO_TEST_DATA_DIR");
    let identifier = scoped_identifier(
        &context.config().identifier,
        cfg!(debug_assertions),
        test_dir.as_deref().map(Path::new),
    )?;
    context.config_mut().identifier = identifier;
    Ok(context)
}

fn scoped_identifier(
    identifier: &str,
    debug_build: bool,
    test_dir: Option<&Path>,
) -> Result<String, &'static str> {
    let Some(test_dir) = test_dir.filter(|_| debug_build) else {
        return Ok(identifier.to_owned());
    };
    if !test_dir.is_absolute() {
        return Err("FONO_TEST_DATA_DIR must be absolute");
    }
    // Match app_data_dir's debug-only creation; canonicalization also makes path
    // aliases share one instance instead of writing to the same profile twice.
    std::fs::create_dir_all(test_dir).map_err(|_| "could not prepare isolated instance scope")?;
    let canonical = test_dir
        .canonicalize()
        .map_err(|_| "could not resolve isolated instance scope")?;
    let digest = Sha256::digest(canonical.as_os_str().as_encoded_bytes());
    Ok(format!("{identifier}.test-{digest:x}"))
}

/// Only the hash enters the Windows object namespace; no profile path is exposed.
#[cfg(windows)]
fn gate_name(identifier: &str) -> String {
    let digest = Sha256::digest(identifier.as_bytes());
    format!("Local\\Fono.Startup.{digest:x}")
}

/// Win32 mutex ownership belongs to the acquiring thread. Rc's marker prevents
/// moving the guard to another thread (or sharing it), even on non-Windows builds.
pub(crate) struct StartupGuard {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    _owning_thread: PhantomData<Rc<()>>,
}

pub(crate) fn startup_gate(identifier: &str) -> Result<StartupGuard, &'static str> {
    #[cfg(windows)]
    {
        let guard = acquire_startup_gate(identifier, 30_000)?;
        // Plugin 2.4.3 forwards the launch but does not delegate the secondary
        // process's foreground permission. Do so before its setup exits us.
        allow_primary_foreground(identifier);
        Ok(guard)
    }
    #[cfg(not(windows))]
    {
        let _ = identifier;
        Ok(StartupGuard {
            _owning_thread: PhantomData,
        })
    }
}

#[cfg(windows)]
fn foreground_target_process(own_pid: u32, primary_pid: u32) -> Option<u32> {
    // u32::MAX is ASFW_ANY: never grant foreground permission to every process.
    (primary_pid != 0 && primary_pid != own_pid && primary_pid != u32::MAX).then_some(primary_pid)
}

#[cfg(windows)]
fn allow_primary_foreground(identifier: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId,
    };

    // Exact private target names from pinned tauri-plugin-single-instance 2.4.3.
    // The startup gate guarantees an earlier Fono has finished publishing it.
    let class: Vec<u16> = format!("{identifier}-sic")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = format!("{identifier}-siw")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are NUL-terminated and remain alive for the call.
    let Ok(window) = (unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(name.as_ptr())) }) else {
        return; // First instance: no existing process needs permission.
    };
    let mut primary_pid = 0;
    // SAFETY: the PID output points to a valid local value. A window disappearing
    // during lookup leaves zero and must never become a wildcard permission.
    unsafe { GetWindowThreadProcessId(window, Some(&mut primary_pid)) };
    if let Some(primary_pid) = foreground_target_process(std::process::id(), primary_pid) {
        // Permission can be refused if the user changed foreground applications.
        // Still let the plugin forward/show the existing window in that case.
        if unsafe { AllowSetForegroundWindow(primary_pid) }.is_err() {
            tracing::debug!("could not delegate foreground activation to running Fono");
        }
    }
}

#[cfg(windows)]
fn acquire_startup_gate(identifier: &str, timeout_ms: u32) -> Result<StartupGuard, &'static str> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};

    let name: Vec<u16> = gate_name(identifier)
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: the name is NUL-terminated and alive for the duration of the call.
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
        .map_err(|_| "could not create application startup gate")?;
    // SAFETY: CreateMutexW returned a valid handle owned by this function.
    let result = unsafe { WaitForSingleObject(handle, timeout_ms) };
    if result == WAIT_OBJECT_0 || result == WAIT_ABANDONED {
        return Ok(StartupGuard {
            handle,
            _owning_thread: PhantomData,
        });
    }
    // A failed/timed-out wait did not acquire ownership. Never continue startup.
    let _ = unsafe { CloseHandle(handle) };
    if result == WAIT_TIMEOUT {
        Err("another application instance is still starting")
    } else {
        Err("could not acquire application startup gate")
    }
}

#[cfg(windows)]
impl Drop for StartupGuard {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::ReleaseMutex;

        // SAFETY: this guard is created only after ownership was acquired; the
        // !Send marker ensures Drop runs on its acquiring thread exactly once.
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

pub(crate) fn plugin<R: Runtime>() -> TauriPlugin<R> {
    // The plugin already forwards argv (including the executable) and cwd to
    // this callback. Keep the hook for F-04; until CLI commands are implemented,
    // discard them without logging, persistence, events or command execution.
    tauri_plugin_single_instance::init(|app, _args, _cwd| {
        let Some(window) = app.get_webview_window("settings") else {
            tracing::warn!("could not find settings window after a second launch");
            return;
        };
        if window.show().is_err() {
            tracing::warn!("could not show settings window after a second launch");
        }
        if window.unminimize().is_err() {
            tracing::warn!("could not restore settings window after a second launch");
        }
        if window.set_focus().is_err() {
            tracing::warn!("could not focus settings window after a second launch");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("fono-instance-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let temp = std::env::temp_dir().canonicalize().unwrap();
            if let Ok(root) = self.0.canonicalize() {
                assert!(root.starts_with(&temp) && root != temp);
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }

    #[test]
    fn release_ignores_test_scope_and_normal_launch_keeps_its_identity() {
        let relative = Path::new("relative-must-not-be-created");
        assert_eq!(
            scoped_identifier("com.fono.desktop", false, Some(relative)).unwrap(),
            "com.fono.desktop"
        );
        assert_eq!(
            scoped_identifier("com.fono.desktop", true, None).unwrap(),
            "com.fono.desktop"
        );
        assert_eq!(
            scoped_identifier("com.fono.desktop", true, Some(relative)),
            Err("FONO_TEST_DATA_DIR must be absolute")
        );
    }

    #[test]
    fn debug_profiles_are_independent_but_aliases_share_the_same_instance() {
        let root = TestDirectory::new();
        let profile = root.0.join("private-profile-name");
        let first = scoped_identifier("com.fono.desktop", true, Some(&profile)).unwrap();
        let alias = scoped_identifier("com.fono.desktop", true, Some(&profile.join("."))).unwrap();
        let other =
            scoped_identifier("com.fono.desktop", true, Some(&root.0.join("other"))).unwrap();
        assert_eq!(first, alias);
        assert_ne!(first, other);
        assert_ne!(first, "com.fono.desktop");
        assert!(!first.contains("private-profile-name"));
        assert_eq!(first.len(), "com.fono.desktop.test-".len() + 64);
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_case_aliases_cannot_create_two_writers() {
        let root = TestDirectory::new();
        let profile = root.0.join("MixedCaseProfile");
        let first = scoped_identifier("com.fono.desktop", true, Some(&profile)).unwrap();
        let different_case = PathBuf::from(profile.as_os_str().to_str().unwrap().to_uppercase());
        let second = scoped_identifier("com.fono.desktop", true, Some(&different_case)).unwrap();
        assert_eq!(first, second);
    }

    #[cfg(windows)]
    #[test]
    fn foreground_permission_targets_only_an_existing_other_process() {
        assert_eq!(foreground_target_process(100, 200), Some(200));
        assert_eq!(foreground_target_process(100, 0), None);
        assert_eq!(foreground_target_process(100, 100), None);
        assert_eq!(foreground_target_process(100, u32::MAX), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_parallel_launch_waits_until_the_first_build_has_published_its_window() {
        use std::sync::mpsc;
        use std::time::Duration;

        let identifier = format!("fono-test-{}", uuid::Uuid::new_v4());
        let first = acquire_startup_gate(&identifier, 1000).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _second = acquire_startup_gate(&identifier, 5000).unwrap();
            acquired_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(acquired_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(first);
        acquired_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_timeout_fails_closed_and_a_different_profile_can_start() {
        let identifier = format!("fono-test-{}", uuid::Uuid::new_v4());
        let first = acquire_startup_gate(&identifier, 1000).unwrap();
        let worker_key = identifier.clone();
        let worker = std::thread::spawn(move || {
            assert!(matches!(
                acquire_startup_gate(&worker_key, 25),
                Err("another application instance is still starting")
            ));
            let _other = acquire_startup_gate(&format!("{worker_key}-other"), 1000).unwrap();
        });
        worker.join().unwrap();
        drop(first);
        let _retry = acquire_startup_gate(&identifier, 1000).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn an_abandoned_startup_gate_allows_recovery_after_a_thread_exits() {
        let identifier = format!("fono-test-{}", uuid::Uuid::new_v4());
        let worker_key = identifier.clone();
        let handle = std::thread::spawn(move || {
            let abandoned = acquire_startup_gate(&worker_key, 1000).unwrap();
            let handle = abandoned.handle.0 as usize;
            std::mem::forget(abandoned);
            handle
        })
        .join()
        .unwrap();
        let recovered = acquire_startup_gate(&identifier, 1000);
        // The thread deliberately skipped Drop to simulate termination while
        // owning the mutex. Close that original fixture handle after recovery.
        let _ = unsafe {
            windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(
                handle as *mut std::ffi::c_void,
            ))
        };
        drop(recovered.unwrap());
    }
}
