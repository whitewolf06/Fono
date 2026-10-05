use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::*,
        UI::{
            Controls::*, HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::EnableWindow,
            WindowsAndMessaging::*,
        },
    },
};

pub const PRIMARY: u16 = 1001;
pub const SECONDARY: u16 = 1002;
pub const BACKGROUND: COLORREF = rgb(23, 27, 33);
pub const TEXT: COLORREF = rgb(229, 234, 242);
pub const MUTED: COLORREF = rgb(159, 170, 185);
pub const ACCENT: COLORREF = rgb(88, 187, 236);

pub const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | (green as u32) << 8 | (blue as u32) << 16)
}

pub struct Controls {
    title: HWND,
    subtitle: HWND,
    pub version: HWND,
    description: HWND,
    pub status: HWND,
    pub progress: HWND,
    pub details: HWND,
    note: HWND,
    pub primary: HWND,
    pub secondary: HWND,
    fonts: Vec<HFONT>,
    pub brush: HBRUSH,
}

impl Controls {
    pub unsafe fn create(parent: HWND, instance: HINSTANCE) -> windows::core::Result<Self> {
        let label = |text: &str| child(parent, instance, w!("STATIC"), text, WINDOW_STYLE(0), 0);
        let title = label("Fono")?;
        let subtitle = label("Онлайн-установка")?;
        let version = label(&format!(
            "Загрузчик {}",
            fono_setup::config::bootstrap_version()
        ))?;
        let description = label("Нужен интернет. Скачаем полный пакет Fono с GitHub Releases и проверим его подпись перед установкой.")?;
        let status = label("Готов к установке")?;
        let progress = child(
            parent,
            instance,
            PROGRESS_CLASSW,
            "",
            WINDOW_STYLE(PBS_SMOOTH),
            0,
        )?;
        let details = child(
            parent,
            instance,
            w!("EDIT"),
            "Нажмите «Установить», чтобы получить последнюю опубликованную версию. После загрузки откроется обычный мастер установки Windows.",
            WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY) as u32) | WS_VSCROLL | WS_TABSTOP,
            0,
        )?;
        let note = label("Модели распознавания скачиваются отдельно в приложении.")?;
        let button_style = WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP;
        let primary = child(
            parent,
            instance,
            w!("BUTTON"),
            "Установить",
            button_style,
            PRIMARY,
        )?;
        let secondary = child(
            parent,
            instance,
            w!("BUTTON"),
            "Закрыть",
            button_style,
            SECONDARY,
        )?;
        let _ = SetWindowTheme(progress, w!(""), w!(""));
        SendMessageW(progress, PBM_SETRANGE32, WPARAM(0), LPARAM(1000));
        SendMessageW(
            progress,
            PBM_SETBARCOLOR,
            WPARAM(0),
            LPARAM(ACCENT.0 as isize),
        );
        SendMessageW(
            progress,
            PBM_SETBKCOLOR,
            WPARAM(0),
            LPARAM(rgb(38, 47, 59).0 as isize),
        );
        let mut controls = Self {
            title,
            subtitle,
            version,
            description,
            status,
            progress,
            details,
            note,
            primary,
            secondary,
            fonts: Vec::new(),
            brush: CreateSolidBrush(BACKGROUND),
        };
        controls.layout(parent);
        Ok(controls)
    }

    pub unsafe fn layout(&mut self, parent: HWND) {
        let dpi = GetDpiForWindow(parent).max(96);
        let scale = |value: i32| value * dpi as i32 / 96;
        let positions = [
            (self.title, 80, 24, 500, 40),
            (self.subtitle, 80, 64, 500, 24),
            (self.version, 28, 96, 564, 22),
            (self.description, 28, 124, 564, 44),
            (self.status, 28, 184, 564, 24),
            (self.progress, 28, 219, 564, 14),
            (self.details, 28, 247, 564, 69),
            (self.note, 28, 327, 564, 22),
            (self.secondary, 28, 366, 128, 40),
            (self.primary, 418, 366, 174, 40),
        ];
        let previous = std::mem::take(&mut self.fonts);
        self.fonts = [32, 16, 14]
            .map(|size| {
                CreateFontW(
                    -scale(size),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5,
                    0,
                    w!("Segoe UI"),
                )
            })
            .to_vec();
        for (window, x, y, width, height) in positions {
            let _ = MoveWindow(
                window,
                scale(x),
                scale(y),
                scale(width),
                scale(height),
                true,
            );
            let font = if window == self.title {
                self.fonts[0]
            } else if window == self.version || window == self.note {
                self.fonts[2]
            } else {
                self.fonts[1]
            };
            SendMessageW(window, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
        for font in previous {
            let _ = DeleteObject(font);
        }
    }

    pub unsafe fn actions(
        &self,
        primary: &str,
        primary_enabled: bool,
        secondary: &str,
        secondary_enabled: bool,
    ) {
        set_text(self.primary, primary);
        set_text(self.secondary, secondary);
        let _ = EnableWindow(self.primary, primary_enabled);
        let _ = EnableWindow(self.secondary, secondary_enabled);
        let _ = InvalidateRect(self.primary, None, true);
        let _ = InvalidateRect(self.secondary, None, true);
    }

    pub unsafe fn draw_button(&self, item: &DRAWITEMSTRUCT) {
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
        let primary = item.CtlID == u32::from(PRIMARY);
        let fill = if disabled {
            rgb(30, 35, 42)
        } else if selected {
            rgb(34, 65, 83)
        } else if primary {
            rgb(30, 52, 67)
        } else {
            rgb(33, 39, 48)
        };
        let brush = CreateSolidBrush(fill);
        FillRect(item.hDC, &item.rcItem, brush);
        let _ = DeleteObject(brush);
        let border = CreateSolidBrush(if primary && !disabled {
            rgb(48, 119, 154)
        } else {
            rgb(62, 72, 85)
        });
        FrameRect(item.hDC, &item.rcItem, border);
        let _ = DeleteObject(border);
        SetBkMode(item.hDC, TRANSPARENT);
        SetTextColor(
            item.hDC,
            if disabled {
                MUTED
            } else if primary {
                ACCENT
            } else {
                TEXT
            },
        );
        let font = SendMessageW(item.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0));
        let old = SelectObject(item.hDC, HGDIOBJ(font.0 as *mut _));
        let mut text = vec![0; 128];
        let length = GetWindowTextW(item.hwndItem, &mut text) as usize;
        text.truncate(length);
        let mut rect = item.rcItem;
        DrawTextW(
            item.hDC,
            &mut text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        SelectObject(item.hDC, old);
        if item.itemState.0 & ODS_FOCUS.0 != 0 {
            rect.left += 4;
            rect.top += 4;
            rect.right -= 4;
            rect.bottom -= 4;
            let _ = DrawFocusRect(item.hDC, &rect);
        }
    }

    pub unsafe fn static_color(&self, hdc: HDC, window: HWND) -> LRESULT {
        SetTextColor(
            hdc,
            if window == self.title || window == self.status {
                TEXT
            } else {
                MUTED
            },
        );
        SetBkColor(hdc, BACKGROUND);
        LRESULT(self.brush.0 as isize)
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        unsafe {
            for font in &self.fonts {
                let _ = DeleteObject(*font);
            }
            let _ = DeleteObject(self.brush);
        }
    }
}

pub unsafe fn set_text(window: HWND, text: &str) {
    let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let _ = SetWindowTextW(window, PCWSTR(text.as_ptr()));
}

unsafe fn child(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    id: u16,
) -> windows::core::Result<HWND> {
    let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(text.as_ptr()),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        0,
        0,
        parent,
        HMENU(id as usize as *mut _),
        instance,
        None,
    )
}
