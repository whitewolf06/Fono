//! Transient basename only. No title, URL, field text or retained process path.
use super::TextTarget;

#[cfg(any(windows, test))]
fn verified_basename(
    target: &TextTarget,
    mut foreground: impl FnMut() -> Option<(isize, u32)>,
    image: impl FnOnce(u32) -> Option<String>,
) -> Option<String> {
    let expected = Some((target.window, target.process));
    if foreground() != expected {
        return None;
    }
    let image = image(target.process)?;
    if foreground() != expected {
        return None;
    }
    let name = image.rsplit(['\\', '/']).next()?;
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(not(windows))]
pub(crate) fn for_target(_: &TextTarget) -> Option<String> {
    None
}

#[cfg(windows)]
pub(crate) fn for_target(target: &TextTarget) -> Option<String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    };
    if target.process == std::process::id() {
        return None;
    }
    verified_basename(
        target,
        || unsafe {
            let window = GetForegroundWindow();
            if window.0.is_null() {
                return None;
            }
            let mut process = 0;
            (GetWindowThreadProcessId(window, Some(&mut process)) != 0)
                .then_some((window.0 as isize, process))
        },
        |process| unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process).ok()?;
            let mut buffer = vec![0u16; 32768];
            let mut size = buffer.len() as u32;
            let result = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(handle);
            result.ok()?;
            String::from_utf16(&buffer[..size as usize]).ok()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> TextTarget {
        TextTarget {
            window: 4,
            process: 5,
            focus: 6,
            runtime_id: vec![7],
        }
    }
    #[test]
    fn only_basename_leaves_identity_probe_and_foreground_change_fails_closed() {
        assert_eq!(
            verified_basename(
                &target(),
                || Some((4, 5)),
                |_| Some("C:\\PRIVATE\\Code.exe".into())
            )
            .as_deref(),
            Some("Code.exe")
        );
        let mut calls = 0;
        assert!(verified_basename(
            &target(),
            || {
                calls += 1;
                Some((if calls == 1 { 4 } else { 9 }, 5))
            },
            |_| Some("C:\\PRIVATE\\Code.exe".into())
        )
        .is_none());
        assert!(verified_basename(
            &target(),
            || Some((4, 8)),
            |_| panic!("different process queried")
        )
        .is_none());
        assert!(verified_basename(&target(), || Some((4, 5)), |_| None).is_none());
    }
}
