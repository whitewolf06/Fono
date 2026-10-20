//! One Win32 owner for clipboard handles, copy and delayed restoration.
//!
//! A successful injection reply means input events were submitted. Windows has
//! no acknowledgement that an arbitrary external application accepted the text.
use super::target::TextTarget;
use crate::{
    application::updates::activity::{self, ActivityLease},
    error::{AppError, AppResult},
};
use crossbeam_channel::{bounded, Receiver, Sender};
use fono_core::OperationCancellation;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::{marker::PhantomData, rc::Rc, time::Duration};
use windows::{
    core::w,
    Win32::{
        Foundation::{
            GetLastError, GlobalFree, SetLastError, ERROR_SUCCESS, HANDLE, HGLOBAL, HWND, LPARAM,
            WPARAM,
        },
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
                GetClipboardOwner, GetClipboardSequenceNumber, OpenClipboard,
                RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
            Ole::{OleDuplicateData, CLIPBOARD_FORMAT},
            Threading::GetCurrentThreadId,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW,
            KillTimer, PostThreadMessageW, SetTimer, TranslateMessage, HWND_MESSAGE, MSG,
            WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_TIMER,
        },
    },
};
mod selection;

const WAKE: u32 = WM_APP + 31;
const RESTORE_TIMER: usize = 1;
const RESTORE_DELAY_MS: u32 = 300;
const MAX_FORMATS: usize = 64;
const MAX_FORMAT_BYTES: usize = 16 * 1024 * 1024;
const MAX_BACKUP_BYTES: usize = 32 * 1024 * 1024;
const PRIVACY_FORMATS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];

type InputGuard = (
    TextTarget,
    OperationCancellation,
    Option<super::target::InputPreflight>,
);
type Guard = Option<InputGuard>;

enum Command {
    Selection {
        target: TextTarget,
        start: u32,
        end: u32,
        cancel: OperationCancellation,
        lease: ActivityLease<'static>,
        reply: Sender<AppResult<String>>,
    },
    Inject {
        text: String,
        guard: Guard,
        lease: ActivityLease<'static>,
        reply: Sender<AppResult<()>>,
    },
    Copy {
        text: String,
        lease: ActivityLease<'static>,
        reply: Sender<AppResult<()>>,
    },
    Shutdown {
        reply: Sender<()>,
    },
}

struct Worker {
    tx: Sender<Command>,
    thread_id: u32,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

static WORKER: Lazy<Result<Worker, String>> = Lazy::new(Worker::start);

fn failure(message: &str) -> AppError {
    AppError::Injection(message.into())
}

fn worker() -> AppResult<&'static Worker> {
    WORKER
        .as_ref()
        .map_err(|_| failure("clipboard owner is unavailable"))
}

pub(super) fn inject(text: &str, guard: Guard) -> AppResult<()> {
    let lease = activity::lease()?;
    let (reply, result) = bounded(1);
    worker()?.post(Command::Inject {
        text: text.into(),
        guard,
        lease,
        reply,
    })?;
    result
        .recv()
        .map_err(|_| failure("clipboard owner disconnected"))?
}

pub(super) fn copy(text: &str) -> AppResult<()> {
    let lease = activity::lease()?;
    let (reply, result) = bounded(1);
    worker()?.post(Command::Copy {
        text: text.into(),
        lease,
        reply,
    })?;
    result
        .recv()
        .map_err(|_| failure("clipboard owner disconnected"))?
}

pub(super) fn selection_text(
    target: &TextTarget,
    start: u32,
    end: u32,
    cancel: OperationCancellation,
) -> AppResult<String> {
    let lease = activity::lease()?;
    let (reply, result) = bounded(1);
    worker()?.post(Command::Selection {
        target: target.clone(),
        start,
        end,
        cancel,
        lease,
        reply,
    })?;
    result
        .recv()
        .map_err(|_| failure("clipboard owner disconnected"))?
}

pub(super) fn shutdown() {
    if let Some(Ok(worker)) = Lazy::get(&WORKER) {
        let (reply, done) = bounded(1);
        let _ = worker.post(Command::Shutdown { reply });
        // Normal shutdown restores the rich backup before the application
        // exits. A permanently busy clipboard remains bounded by retry limits.
        let _ = done.recv_timeout(Duration::from_secs(1));
        // Never destroy Win32 handles on the calling (possibly UI) thread.
        // The owner drains its current cleanup before processing Shutdown.
        if let Some(thread) = worker.thread.lock().take() {
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

impl Worker {
    fn start() -> Result<Self, String> {
        let (tx, rx) = bounded(4);
        let (ready_tx, ready_rx) = bounded::<Result<u32, String>>(1);
        let thread = std::thread::Builder::new()
            .name("fono-clipboard-owner".into())
            .spawn(move || match Owner::new() {
                Ok(mut owner) => {
                    let _ = ready_tx.send(Ok(unsafe { GetCurrentThreadId() }));
                    owner.run(rx);
                }
                Err(_) => {
                    let _ = ready_tx.send(Err("clipboard window could not be created".into()));
                }
            })
            .map_err(|_| "clipboard thread could not be created".to_owned())?;
        let thread_id = ready_rx
            .recv()
            .map_err(|_| "clipboard thread disconnected".to_owned())??;
        Ok(Self {
            tx,
            thread_id,
            thread: Mutex::new(Some(thread)),
        })
    }

    fn post(&self, command: Command) -> AppResult<()> {
        self.tx
            .try_send(command)
            .map_err(|_| failure("clipboard owner queue is busy"))?;
        // Once queued, do not report a failure that could be followed by a late
        // side effect. A full native message queue already wakes the owner; it
        // drains commands on every message. A dead owner drops reply senders.
        if unsafe { PostThreadMessageW(self.thread_id, WAKE, WPARAM(0), LPARAM(0)) }.is_err() {
            tracing::warn!("clipboard owner wake message was unavailable");
        }
        Ok(())
    }
}

struct PendingRestore<B, L> {
    sequence: u32,
    backup: B,
    _lease: L,
    retries: u8,
}

impl<B, L> PendingRestore<B, L> {
    fn still_owned(&self, sequence: u32) -> bool {
        self.sequence != 0 && self.sequence == sequence
    }
    fn record_own_partial_restore(&mut self, sequence: u32, still_owner: bool) -> bool {
        if !still_owner || sequence == 0 {
            return false;
        }
        self.sequence = sequence;
        true
    }
}

struct Owner {
    window: HWND,
    pending: Option<PendingRestore<Backup, ActivityLease<'static>>>,
    // Every handle and window is created, used and destroyed by this thread.
    _not_send: PhantomData<Rc<()>>,
}

impl Owner {
    fn new() -> AppResult<Self> {
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Fono clipboard owner"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                None,
                None,
            )
        }
        .map_err(|_| failure("clipboard window could not be created"))?;
        Ok(Self {
            window,
            pending: None,
            _not_send: PhantomData,
        })
    }

    fn run(&mut self, rx: Receiver<Command>) {
        let mut message = MSG::default();
        let mut shutting_down = false;
        let mut shutdown_replies = Vec::new();
        loop {
            let status = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
            if status <= 0 {
                break;
            }
            while let Ok(command) = rx.try_recv() {
                match command {
                    Command::Selection {
                        target,
                        start,
                        end,
                        cancel,
                        lease,
                        reply,
                    } => {
                        let result = if shutting_down {
                            Err(failure("clipboard owner is shutting down"))
                        } else {
                            self.selection_text(&target, start, end, &cancel, lease)
                        };
                        let _ = reply.send(result);
                    }
                    Command::Inject {
                        text,
                        guard,
                        lease,
                        reply,
                    } => {
                        if shutting_down {
                            let _ = reply.send(Err(failure("clipboard owner is shutting down")));
                            continue;
                        }
                        let output = self.inject(&text, guard.as_ref(), lease);
                        // No 150/300 ms consumption wait belongs in this reply.
                        let _ = reply.send(output);
                    }
                    Command::Copy { text, lease, reply } => {
                        if shutting_down {
                            let _ = reply.send(Err(failure("clipboard owner is shutting down")));
                            continue;
                        }
                        let result = self.copy(&text, lease);
                        let _ = reply.send(result);
                    }
                    Command::Shutdown { reply } => {
                        shutdown_replies.push(reply);
                        shutting_down = true;
                        self.restore();
                    }
                }
            }
            if message.message == WM_TIMER && message.wParam.0 == RESTORE_TIMER {
                self.restore();
            } else if message.message != WAKE {
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            if shutting_down && self.pending.is_none() {
                for reply in shutdown_replies {
                    let _ = reply.try_send(());
                }
                return;
            }
        }
    }

    fn inject(
        &mut self,
        text: &str,
        guard: Option<&InputGuard>,
        lease: ActivityLease<'static>,
    ) -> AppResult<()> {
        if self.pending.is_some() {
            // Never snapshot our own temporary text as the next user's backup.
            return Err(failure("clipboard restoration is still pending"));
        }
        super::modifiers::wait_for_release(|| {
            guard.is_some_and(|(_, cancel, _)| cancel.is_cancelled())
        })?;
        ensure_target(guard)?;
        let window = unsafe { GetForegroundWindow() };
        if window.0.is_null() {
            return Err(failure("no foreground window for clipboard injection"));
        }
        // Allocate everything before changing the clipboard. If a format cannot
        // be duplicated safely, leave the entire original clipboard untouched.
        let mut data = text_formats(text)?;
        let opened = Opened::new(self.window)?;
        let backup = Backup::capture()?;
        let publication = replace_open(&mut data);
        drop(opened);
        let sequence = unsafe { GetClipboardSequenceNumber() };
        self.pending = Some(PendingRestore {
            sequence,
            backup,
            _lease: lease,
            retries: 0,
        });
        if let Err(error) = publication {
            self.restore();
            return Err(error);
        }
        if sequence == 0
            || unsafe { GetForegroundWindow() } != window
            || ensure_target(guard).is_err()
        {
            self.restore();
            return Err(failure("clipboard destination changed before input"));
        }
        // Set up cleanup before sending anything. Failure cannot leave an
        // untracked clipboard snapshot or release its updater fence early.
        if unsafe { SetTimer(self.window, RESTORE_TIMER, RESTORE_DELAY_MS, None) } == 0 {
            self.restore();
            return Err(failure("clipboard restoration could not be scheduled"));
        }
        if let Err(error) = ensure_target(guard) {
            self.restore();
            return Err(error);
        }
        if let Err(error) = super::modifiers::ensure_released() {
            self.restore();
            return Err(error);
        }
        if guard.is_some_and(|(_, cancel, _)| cancel.is_cancelled())
            || unsafe { GetForegroundWindow() } != window
        {
            self.restore();
            return Err(failure("clipboard injection was cancelled before input"));
        }
        if let Some(check) = guard.and_then(|(_, _, check)| check.as_ref()) {
            if !check() || ensure_target(guard).is_err() {
                self.restore();
                return Err(failure(
                    "Поле или выделение изменилось. Выделите фрагмент заново.",
                ));
            }
        }
        match super::send_ctrl_v() {
            Ok(()) => {
                tracing::debug!("submitted clipboard paste input events");
                Ok(())
            }
            Err(error) => {
                // A partial SendInput is uncertain. Keep the payload available
                // for the same grace interval, and do not automatically retry.
                Err(error)
            }
        }
    }

    fn copy(&mut self, text: &str, lease: ActivityLease<'static>) -> AppResult<()> {
        let mut data = text_formats(text)?;
        let opened = Opened::new(self.window)?;
        // A failing explicit Copy must not destroy the previous rich payload.
        // If injection cleanup is pending, its master is the user's original
        // clipboard, rather than the temporary dictation currently published.
        let backup = match self.pending.take() {
            Some(pending)
                if pending.still_owned(unsafe { GetClipboardSequenceNumber() })
                    && unsafe { GetClipboardOwner() }.ok() == Some(self.window) =>
            {
                pending.backup
            }
            _ => Backup::capture()?,
        };
        let publication = replace_open(&mut data);
        drop(opened);
        if let Err(error) = publication {
            self.pending = Some(PendingRestore {
                sequence: unsafe { GetClipboardSequenceNumber() },
                backup,
                _lease: lease,
                retries: 0,
            });
            self.restore();
            return Err(error);
        }
        // Explicit Copy supersedes temporary injection, including equal text.
        self.pending.take();
        unsafe {
            let _ = KillTimer(self.window, RESTORE_TIMER);
        }
        Ok(())
    }

    fn restore(&mut self) {
        unsafe {
            let _ = KillTimer(self.window, RESTORE_TIMER);
        }
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        let Ok(opened) = Opened::new(self.window) else {
            // A busy clipboard is common during Copy. Retry on the same owner
            // and keep the admission lease until cleanup is actually finished.
            self.retry_restore(pending);
            return;
        };
        if pending.still_owned(unsafe { GetClipboardSequenceNumber() })
            && unsafe { GetClipboardOwner() }.ok() == Some(self.window)
            && pending.backup.restore_open().is_err()
        {
            // Partial restoration modified only our own locked clipboard.
            // Record its new sequence while still holding OpenClipboard.
            // The immutable master remains valid for a complete retry.
            let retry = pending.record_own_partial_restore(
                unsafe { GetClipboardSequenceNumber() },
                unsafe { GetClipboardOwner() }.ok() == Some(self.window),
            );
            drop(opened);
            if retry {
                self.retry_restore(pending);
            }
        }
        // A newer sequence belongs to the user, even if its text is identical.
    }

    fn retry_restore(&mut self, mut pending: PendingRestore<Backup, ActivityLease<'static>>) {
        pending.retries += 1;
        if pending.retries <= 20 && unsafe { SetTimer(self.window, RESTORE_TIMER, 25, None) } != 0 {
            self.pending = Some(pending);
        } else {
            tracing::warn!("clipboard backup restoration remained unavailable");
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.restore();
        unsafe {
            let _ = DestroyWindow(self.window);
        }
    }
}

fn ensure_target(guard: Option<&InputGuard>) -> AppResult<()> {
    if guard
        .is_some_and(|(target, cancel, _)| cancel.is_cancelled() || !super::target::matches(target))
    {
        return Err(failure("clipboard destination is unavailable"));
    }
    Ok(())
}

struct Opened;
impl Opened {
    fn new(window: HWND) -> AppResult<Self> {
        unsafe { OpenClipboard(window) }
            .map_err(|_| failure("clipboard is busy; original content was preserved"))?;
        Ok(Self)
    }
}
impl Drop for Opened {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

struct FormatData {
    format: u32,
    handle: HGLOBAL,
}

impl FormatData {
    fn duplicate(&self) -> AppResult<Self> {
        let handle = unsafe {
            OleDuplicateData(
                HANDLE(self.handle.0),
                CLIPBOARD_FORMAT(self.format as u16),
                GMEM_MOVEABLE,
            )
        };
        if handle.is_invalid() {
            return Err(failure("clipboard format could not be duplicated"));
        }
        Ok(Self {
            format: self.format,
            handle: HGLOBAL(handle.0),
        })
    }
    fn from_bytes(format: u32, bytes: &[u8]) -> AppResult<Self> {
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
            .map_err(|_| failure("clipboard allocation failed"))?;
        let result = Self { format, handle };
        let pointer = unsafe { GlobalLock(handle) };
        if pointer.is_null() {
            return Err(failure("clipboard allocation could not be locked"));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
            let _ = GlobalUnlock(handle);
        }
        Ok(result)
    }

    fn publish(&mut self) -> AppResult<()> {
        unsafe { SetClipboardData(self.format, HANDLE(self.handle.0)) }
            .map_err(|_| failure("clipboard format could not be published"))?;
        // Ownership transfers to Windows only after SetClipboardData succeeds.
        self.handle = HGLOBAL::default();
        Ok(())
    }
}
impl Drop for FormatData {
    fn drop(&mut self) {
        if !self.handle.0.is_null() {
            unsafe {
                let _ = GlobalFree(self.handle);
            }
        }
    }
}

#[derive(Default)]
struct BackupBudget {
    formats: usize,
    bytes: usize,
}

impl BackupBudget {
    fn include(&mut self, format: u32, bytes: usize) -> AppResult<()> {
        // GDI/metafile/owner-display and application-private handles require
        // their own destructor/serialization. Refuse before EmptyClipboard,
        // rather than drop them or treat a native handle as global memory.
        let global_memory = matches!(
            format,
            1 | 4 | 5 | 6 | 7 | 8 | 10 | 11 | 12 | 13 | 15 | 16 | 17
        ) || (0xc000..=0xffff).contains(&format);
        if !global_memory
            || bytes == 0
            || bytes > MAX_FORMAT_BYTES
            || self.formats >= MAX_FORMATS
            || self.bytes.saturating_add(bytes) > MAX_BACKUP_BYTES
        {
            return Err(failure(
                "clipboard backup is unsupported or exceeds its safe limit",
            ));
        }
        self.formats += 1;
        self.bytes += bytes;
        Ok(())
    }
}

struct Backup(Vec<FormatData>);
impl Backup {
    fn capture() -> AppResult<Self> {
        let mut formats = Vec::new();
        let mut budget = BackupBudget::default();
        let mut format = 0;
        loop {
            unsafe { SetLastError(ERROR_SUCCESS) };
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                if unsafe { GetLastError() } != ERROR_SUCCESS {
                    return Err(failure("clipboard formats could not be enumerated"));
                }
                break;
            }
            // Check the handle kind before calling GlobalSize on it.
            budget.include(format, 1)?;
            budget.formats -= 1;
            budget.bytes -= 1;
            let handle = unsafe { GetClipboardData(format) }
                .map_err(|_| failure("clipboard format could not be read"))?;
            let size = unsafe { GlobalSize(HGLOBAL(handle.0)) };
            budget.include(format, size)?;
            let duplicate =
                unsafe { OleDuplicateData(handle, CLIPBOARD_FORMAT(format as u16), GMEM_MOVEABLE) };
            if duplicate.is_invalid() {
                return Err(failure("clipboard format could not be duplicated"));
            }
            formats.push(FormatData {
                format,
                handle: HGLOBAL(duplicate.0),
            });
        }
        Ok(Self(formats))
    }

    fn restore_open(&self) -> AppResult<()> {
        let mut publication = duplicate_master(&self.0, FormatData::duplicate)?;
        replace_open(&mut publication)
    }
}

fn duplicate_master<T, E>(
    master: &[T],
    duplicate: impl Fn(&T) -> Result<T, E>,
) -> Result<Vec<T>, E> {
    master.iter().map(duplicate).collect()
}

fn replace_open(formats: &mut [FormatData]) -> AppResult<()> {
    unsafe { EmptyClipboard() }.map_err(|_| failure("clipboard could not be cleared"))?;
    for format in formats {
        format.publish()?;
    }
    Ok(())
}

fn text_formats(text: &str) -> AppResult<Vec<FormatData>> {
    if text.len() > MAX_FORMAT_BYTES / 2 {
        return Err(failure("clipboard text exceeds its safe limit"));
    }
    let unicode: Vec<u8> = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut formats = Vec::with_capacity(4);
    // Publish all privacy markers before text; monitors never see an earlier
    // clipboard-close containing unmarked dictation text.
    for name in PRIVACY_FORMATS {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let format = unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(wide.as_ptr())) };
        if format == 0 {
            return Err(failure("clipboard privacy format could not be registered"));
        }
        formats.push(FormatData::from_bytes(format, &0u32.to_le_bytes())?);
    }
    formats.push(FormatData::from_bytes(13, &unicode)?);
    Ok(formats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[test]
    fn restore_uses_sequence_identity_even_if_user_copied_equal_text() {
        let pending = PendingRestore {
            sequence: 42,
            backup: (),
            _lease: (),
            retries: 0,
        };
        assert!(pending.still_owned(42));
        assert!(!pending.still_owned(43));
        assert!(!pending.still_owned(0));
        let unavailable = PendingRestore {
            sequence: 0,
            backup: (),
            _lease: (),
            retries: 0,
        };
        assert!(!unavailable.still_owned(0));
    }

    #[test]
    fn backup_keeps_text_html_rtf_and_custom_memory_formats_together() {
        let mut budget = BackupBudget::default();
        for (format, bytes) in [(13, 128), (0xc001, 2048), (0xc002, 4096), (0xc003, 9)] {
            budget.include(format, bytes).unwrap();
        }
        assert_eq!(budget.formats, 4);
        assert_eq!(budget.bytes, 6281);
    }

    #[test]
    fn unsupported_native_handles_and_oversize_formats_abort_before_mutation() {
        for format in [2, 3, 9, 14, 0x80, 0x200, 0x300] {
            assert!(BackupBudget::default().include(format, 12).is_err());
        }
        assert!(BackupBudget::default().include(13, 0).is_err());
        assert!(BackupBudget::default()
            .include(13, MAX_FORMAT_BYTES + 1)
            .is_err());
        let mut count = BackupBudget::default();
        for _ in 0..MAX_FORMATS {
            count.include(13, 1).unwrap();
        }
        assert!(count.include(13, 1).is_err());
        let mut bytes = BackupBudget::default();
        bytes.include(13, MAX_FORMAT_BYTES).unwrap();
        bytes.include(0xc001, MAX_FORMAT_BYTES).unwrap();
        assert!(bytes.include(0xc002, 1).is_err());
    }

    #[test]
    fn submitted_reply_does_not_release_cleanup_lease() {
        struct Lease(Arc<AtomicBool>);
        impl Drop for Lease {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let released = Arc::new(AtomicBool::new(false));
        let pending = PendingRestore {
            sequence: 4,
            backup: vec!["HTML", "RTF"],
            _lease: Lease(released.clone()),
            retries: 0,
        };
        let (tx, rx) = bounded(1);
        tx.send(Ok::<_, ()>(())).unwrap();
        assert!(rx.recv().unwrap().is_ok());
        drop(rx);
        assert!(!released.load(Ordering::Acquire));
        assert_eq!(pending.backup, ["HTML", "RTF"]);
        drop(pending);
        assert!(released.load(Ordering::Acquire));
    }

    #[test]
    fn partial_restore_does_not_consume_master_and_retry_restores_every_format() {
        let master = vec![(13, "plain"), (0xc001, "HTML"), (0xc002, "RTF")];
        let first = duplicate_master(&master, |item| Ok::<_, ()>(*item)).unwrap();
        let mut clipboard = Vec::new();
        for item in first.into_iter().take(2) {
            clipboard.push(item);
        }
        assert_eq!(clipboard.len(), 2);
        assert_eq!(master.len(), 3);
        let second = duplicate_master(&master, |item| Ok::<_, ()>(*item)).unwrap();
        clipboard.clear();
        clipboard.extend(second);
        assert_eq!(clipboard, master);
    }

    #[test]
    fn duplication_failure_happens_before_empty_and_keeps_original_payload() {
        let master = vec![(13, "plain"), (0xc001, "HTML"), (0xc002, "RTF")];
        let clipboard = master.clone();
        let publication =
            duplicate_master(
                &master,
                |item| {
                    if item.0 == 0xc002 {
                        Err(())
                    } else {
                        Ok(*item)
                    }
                },
            );
        assert!(publication.is_err());
        assert_eq!(clipboard, master);
    }

    #[test]
    fn failed_restore_advances_only_our_sequence_and_never_follows_user_copy() {
        let mut pending = PendingRestore {
            sequence: 41,
            backup: (),
            _lease: (),
            retries: 0,
        };
        assert!(pending.record_own_partial_restore(42, true));
        assert!(pending.still_owned(42));
        assert!(!pending.record_own_partial_restore(43, false));
        assert!(!pending.still_owned(43));
        assert!(!pending.record_own_partial_restore(0, true));
        assert!(pending.still_owned(42));
    }

    #[test]
    fn injected_payload_has_all_three_windows_privacy_markers() {
        assert_eq!(
            PRIVACY_FORMATS,
            [
                "ExcludeClipboardContentFromMonitorProcessing",
                "CanIncludeInClipboardHistory",
                "CanUploadToCloudClipboard",
            ]
        );
    }
}
