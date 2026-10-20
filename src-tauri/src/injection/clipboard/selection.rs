//! Synchronous temporary Copy on the existing owner. Reply follows restoration.
use super::*;
use crate::injection::selection::{matches_classic, validate_text, MAX_UTF16};
use std::time::Instant;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsChild};

#[derive(Clone, Copy)]
struct Stamp {
    sequence: u32,
    owner: HWND,
}
impl Stamp {
    fn matches(self, sequence: u32, owner: HWND) -> bool {
        self.sequence != 0 && self.sequence == sequence && self.owner == owner
    }
}

fn temporary_copy<T, G>(
    mut stamp: Stamp,
    _guard: G,
    cancelled: impl Fn() -> bool,
    read: impl FnOnce(&mut Stamp) -> AppResult<T>,
    restore: impl FnOnce(Stamp) -> AppResult<()>,
) -> AppResult<T> {
    let result = read(&mut stamp);
    restore(stamp)?;
    if cancelled() {
        return Err(failure("Получение выделения отменено."));
    }
    result
}

impl Owner {
    pub(super) fn selection_text(
        &mut self,
        target: &TextTarget,
        start: u32,
        end: u32,
        cancel: &OperationCancellation,
        lease: ActivityLease<'static>,
    ) -> AppResult<String> {
        self.restore();
        if self.pending.is_some() {
            return Err(failure(
                "Предыдущее восстановление буфера ещё не завершено.",
            ));
        }
        super::super::modifiers::wait_for_release(|| cancel.is_cancelled())?;
        if cancel.is_cancelled() || !matches_classic(target, start, end) {
            return Err(failure(
                "Поле или выделение изменилось. Выделите фрагмент заново.",
            ));
        }
        let mut marker = text_formats("")?;
        let opened = Opened::new(self.window)?;
        let backup = Backup::capture()?;
        let publication = replace_open(&mut marker);
        drop(opened);
        let stamp = Stamp {
            sequence: unsafe { GetClipboardSequenceNumber() },
            owner: self.window,
        };
        temporary_copy(
            stamp,
            lease,
            || cancel.is_cancelled(),
            |stamp| {
                publication?;
                if cancel.is_cancelled() || !matches_classic(target, start, end) {
                    return Err(failure(
                        "Поле или выделение изменилось. Выделите фрагмент заново.",
                    ));
                }
                let submitted = super::super::send_ctrl_key(0x43);
                // Even a partial Ctrl+C can be followed by a copy. Drain that interval
                // on Cancel before restoring; never retry input events.
                let deadline = Instant::now() + Duration::from_millis(500);
                loop {
                    let sequence = unsafe { GetClipboardSequenceNumber() };
                    if sequence != 0 && sequence != stamp.sequence {
                        let owner = unsafe { GetClipboardOwner() }.unwrap_or_default();
                        if !copy_owner_matches(owner, target) {
                            return Err(failure(
                                "Буфер изменился другим действием. Новое содержимое сохранено.",
                            ));
                        }
                        *stamp = Stamp { sequence, owner };
                        submitted?;
                        if cancel.is_cancelled() {
                            return Err(failure("Получение выделения отменено."));
                        }
                        if !matches_classic(target, start, end) {
                            return Err(failure(
                                "Поле или выделение изменилось. Выделите фрагмент заново.",
                            ));
                        }
                        let opened = Opened::new(self.window)?;
                        if !stamp.matches(
                            unsafe { GetClipboardSequenceNumber() },
                            unsafe { GetClipboardOwner() }.unwrap_or_default(),
                        ) {
                            return Err(failure(
                                "Буфер изменился другим действием. Новое содержимое сохранено.",
                            ));
                        }
                        let text = read_unicode()?;
                        drop(opened);
                        if cancel.is_cancelled() || !matches_classic(target, start, end) {
                            return Err(failure(
                                "Поле или выделение изменилось. Выделите фрагмент заново.",
                            ));
                        }
                        return validate_text(text);
                    }
                    if Instant::now() >= deadline {
                        return Err(failure("Приложение не передало выделенный текст в буфер."));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            },
            |stamp| self.restore_selection(&backup, stamp),
        )
    }

    fn restore_selection(&self, backup: &Backup, mut stamp: Stamp) -> AppResult<()> {
        for _ in 0..20 {
            if let Ok(opened) = Opened::new(self.window) {
                if !stamp.matches(
                    unsafe { GetClipboardSequenceNumber() },
                    unsafe { GetClipboardOwner() }.unwrap_or_default(),
                ) {
                    return Ok(()); // A newer user copy wins, including equal text.
                }
                if backup.restore_open().is_ok() {
                    return Ok(());
                }
                // Only our own partial publication may advance the retry stamp.
                if unsafe { GetClipboardOwner() }.ok() != Some(self.window) {
                    return Ok(());
                }
                stamp = Stamp {
                    sequence: unsafe { GetClipboardSequenceNumber() },
                    owner: self.window,
                };
                drop(opened);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Err(failure("Не удалось восстановить буфер после получения выделения. Проверьте буфер перед продолжением."))
    }
}

fn copy_owner_matches(owner: HWND, target: &TextTarget) -> bool {
    let mut process = 0;
    unsafe {
        GetWindowThreadProcessId(owner, Some(&mut process));
    }
    process == target.process
        && (owner.0 as isize == target.window
            || owner.0 as isize == target.focus
            || unsafe { IsChild(HWND(target.window as *mut _), owner) }.as_bool())
}

fn read_unicode() -> AppResult<String> {
    let handle = unsafe { GetClipboardData(13) }
        .map_err(|_| failure("Приложение не передало выделенный текст в буфер."))?;
    let handle = HGLOBAL(handle.0);
    let bytes = unsafe { GlobalSize(handle) };
    if bytes < 2 || bytes % 2 != 0 || bytes > (MAX_UTF16 + 1) * 2 + 16 {
        return Err(failure(
            "Выделение слишком большое. Выберите фрагмент до 32 тысяч символов.",
        ));
    }
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        return Err(failure("Не удалось прочитать выделенный фрагмент."));
    }
    let result = (|| {
        let units = unsafe { std::slice::from_raw_parts(pointer.cast::<u16>(), bytes / 2) };
        let end = units
            .iter()
            .position(|unit| *unit == 0)
            .ok_or_else(|| failure("Не удалось прочитать выделенный фрагмент."))?;
        String::from_utf16(&units[..end])
            .map_err(|_| failure("Выделение содержит неподдерживаемые символы."))
    })();
    unsafe {
        let _ = GlobalUnlock(handle);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    #[test]
    fn cancelled_copy_restores_all_formats_before_reply_and_guard_release() {
        let original = vec!["text", "HTML", "RTF", "registered format"];
        let clipboard = RefCell::new(original.clone());
        let cancelled = Cell::new(false);
        let dropped = Arc::new(AtomicBool::new(false));
        let result = temporary_copy(
            Stamp {
                sequence: 17,
                owner: HWND(1usize as *mut _),
            },
            Guard(dropped.clone()),
            || cancelled.get(),
            |stamp| {
                *clipboard.borrow_mut() = vec!["selected text"];
                stamp.sequence = 18;
                cancelled.set(true);
                Ok("selected text")
            },
            |stamp| {
                assert_eq!(stamp.sequence, 18);
                assert!(!dropped.load(Ordering::SeqCst));
                *clipboard.borrow_mut() = original.clone();
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(*clipboard.borrow(), original);
        assert!(dropped.load(Ordering::SeqCst));
    }
    #[test]
    fn read_failure_also_restores_and_cleanup_failure_is_never_success() {
        let restored = Cell::new(false);
        let stamp = Stamp {
            sequence: 1,
            owner: HWND::default(),
        };
        let result: AppResult<()> = temporary_copy(
            stamp,
            (),
            || false,
            |_| Err(failure("read failed")),
            |_| {
                restored.set(true);
                Ok(())
            },
        );
        assert!(restored.get());
        assert!(result.is_err());
        let result = temporary_copy(
            stamp,
            (),
            || false,
            |_| Ok("text"),
            |_| Err(failure("restore failed")),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "ошибка вставки текста: restore failed"
        );
    }
    #[test]
    fn restore_never_follows_a_later_copy_even_if_its_text_would_match() {
        let stamp = Stamp {
            sequence: 17,
            owner: HWND(1usize as *mut _),
        };
        assert!(stamp.matches(17, HWND(1usize as *mut _)));
        assert!(!stamp.matches(18, HWND(1usize as *mut _)));
        assert!(!stamp.matches(17, HWND(2usize as *mut _)));
        assert!(!Stamp {
            sequence: 0,
            ..stamp
        }
        .matches(0, stamp.owner));
    }
}
