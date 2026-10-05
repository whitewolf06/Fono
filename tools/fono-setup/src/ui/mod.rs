mod controls;
mod state;
mod worker;

use state::App;

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::*,
            HiDpi::*,
            Input::KeyboardAndMouse::{IsWindowEnabled, SetFocus},
            WindowsAndMessaging::*,
        },
    },
};

use controls::{Controls, PRIMARY, SECONDARY};

const CLASS: PCWSTR = w!("FonoOnlineSetup");
const TIMER: usize = 1;

pub fn run() -> windows::core::Result<()> {
    unsafe {
        // Optional on older Windows; failure does not prevent installation.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS,
        })
        .ok()?;
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let icon = LoadIconW(instance, PCWSTR(101usize as *const u16)).unwrap_or_default();
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hIcon: icon,
            hIconSm: icon,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(windows::core::Error::from_win32());
        }
        let style = WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX;
        let dpi = GetDpiForSystem().max(96);
        let mut rect = RECT {
            right: 620 * dpi as i32 / 96,
            bottom: 430 * dpi as i32 / 96,
            ..Default::default()
        };
        AdjustWindowRectExForDpi(&mut rect, style, false, WINDOW_EX_STYLE(0), dpi)?;
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut work as *mut RECT).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let mut app = Box::<App>::default();
        let window = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            w!("Установка Fono"),
            style,
            work.left + ((work.right - work.left - (rect.right - rect.left)) / 2).max(0),
            work.top + ((work.bottom - work.top - (rect.bottom - rect.top)) / 2).max(0),
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            instance,
            Some((&mut *app as *mut App).cast()),
        )?;
        if let Some(error) = app.startup_error.take() {
            return Err(error);
        }
        let _ = ShowWindow(window, SW_SHOWNORMAL);
        if let Some(controls) = &app.controls {
            let _ = SetFocus(controls.primary);
        }
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == -1 {
                return Err(windows::core::Error::from_win32());
            }
            if result == 0 {
                break;
            }
            if !IsDialogMessageW(window, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }
}

pub fn show_startup_error(message: &str) {
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("Не удалось открыть установщик Fono"),
            MB_OK | MB_ICONERROR,
        );
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut App;
    if pointer.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    let app = &mut *pointer;
    match message {
        WM_CREATE => {
            let instance = HINSTANCE(GetWindowLongPtrW(window, GWLP_HINSTANCE) as *mut _);
            match Controls::create(window, instance) {
                Ok(controls) => app.controls = Some(controls),
                Err(error) => {
                    app.startup_error = Some(error);
                    return LRESULT(-1);
                }
            }
            if SetTimer(window, TIMER, 100, None) == 0 {
                app.startup_error = Some(windows::core::Error::from_win32());
                return LRESULT(-1);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wparam.0 as u16 {
                PRIMARY
                    if app.download.is_none()
                        && app.controls.as_ref().is_some_and(|controls| {
                            IsWindowEnabled(controls.primary).as_bool()
                        }) =>
                {
                    app.start()
                }
                // IsDialogMessage sends standard IDCANCEL (2) for Escape.
                SECONDARY | 2 => app.close_or_cancel(window, false),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            app.close_or_cancel(window, true);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER => {
            app.receive(window);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => app
            .controls
            .as_ref()
            .map(|controls| {
                controls.static_color(HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _))
            })
            .unwrap_or_default(),
        WM_DRAWITEM => {
            if let Some(controls) = &app.controls {
                controls.draw_button(&*(lparam.0 as *const DRAWITEMSTRUCT));
            }
            LRESULT(1)
        }
        WM_ERASEBKGND => {
            if let Some(controls) = &app.controls {
                let mut rect = RECT::default();
                let _ = GetClientRect(window, &mut rect);
                FillRect(HDC(wparam.0 as *mut _), &rect, controls.brush);
            }
            LRESULT(1)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(window, &mut paint);
            let dpi = GetDpiForWindow(window).max(96) as i32;
            let instance = HINSTANCE(GetWindowLongPtrW(window, GWLP_HINSTANCE) as *mut _);
            if let Ok(icon) = LoadIconW(instance, PCWSTR(101usize as *const u16)) {
                let _ = DrawIconEx(
                    dc,
                    28 * dpi / 96,
                    34 * dpi / 96,
                    icon,
                    36 * dpi / 96,
                    36 * dpi / 96,
                    0,
                    None,
                    DI_NORMAL,
                );
            }
            let _ = EndPaint(window, &paint);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let rect = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                window,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            if let Some(controls) = &mut app.controls {
                controls.layout(window);
            }
            LRESULT(0)
        }
        DM_GETDEFID => LRESULT(((DC_HASDEFID as usize) << 16 | PRIMARY as usize) as isize),
        WM_DESTROY => {
            let _ = KillTimer(window, TIMER);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}
