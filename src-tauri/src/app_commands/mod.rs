//! Голосовые команды для управления приложениями Windows.
//!
//! Поддерживаемые префиксы:
//! - "переключись на ..." / "перейди в ..." / "открой ..." — активировать уже
//!   запущенное окно по заголовку.
//! - "запусти ..." — запустить приложение из настроенного списка `launch_apps`.

use std::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::types::LaunchApp;

#[cfg(windows)]
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};

/// Пытается выполнить голосовую команду.
pub fn execute(text: &str, launch_apps: &[LaunchApp], volume_step: u32) -> AppResult<String> {
    let normalized = normalize(text);
    crate::vlog!("voice command normalized: {}", normalized);

    if let Some(query) = strip_prefixes(
        &normalized,
        &[
            "переключись на",
            "перейди в",
            "перейди на",
            "открой",
            "включи",
        ],
    ) {
        let query = query.trim();
        if query.is_empty() {
            return Err(AppError::Config(
                "не указано имя окна для переключения".into(),
            ));
        }
        let title = switch_to_window(query)?;
        return Ok(format!("Переключился на «{title}»"));
    }

    if let Some(query) = strip_prefixes(&normalized, &["запусти", "старт", "открыть"])
    {
        let query = query.trim();
        if query.is_empty() {
            return Err(AppError::Config(
                "не указано имя приложения для запуска".into(),
            ));
        }
        let name = launch_application(query, launch_apps)?;
        return Ok(format!("Запустил «{name}»"));
    }

    // Системные и медиа-команды.
    // Стандартный media-key шаг Windows ~2%, поэтому повторяем нажатие
    // нужное количество раз, чтобы набрать заданный `volume_step`.
    let volume_presses = (volume_step.max(2) / 2).max(1) as usize;
    if normalized.contains("громче") {
        for _ in 0..volume_presses {
            send_media_key(0xAF)?;
        }
        return Ok(format!("Громкость +{volume_step}%"));
    }
    if normalized.contains("тише") {
        for _ in 0..volume_presses {
            send_media_key(0xAE)?;
        }
        return Ok(format!("Громкость −{volume_step}%"));
    }
    if normalized.contains("выключи звук")
        || normalized.contains("mute")
        || normalized.contains("без звука")
    {
        send_media_key(0xAD)?;
        return Ok("Звук выключен".to_string());
    }
    if normalized.contains("пауза")
        || normalized.contains("play")
        || normalized.contains("воспроизведение")
    {
        send_media_key(0xB3)?;
        return Ok("Play/Pause".to_string());
    }
    if normalized.contains("следующий")
        || normalized.contains("вперёд")
        || normalized.contains("вперед")
    {
        send_media_key(0xB0)?;
        return Ok("Следующий трек".to_string());
    }
    if normalized.contains("предыдущий") || normalized.contains("назад") {
        send_media_key(0xB1)?;
        return Ok("Предыдущий трек".to_string());
    }

    Err(AppError::Config(format!(
        "команда не распознана: \"{text}\""
    )))
}

/// Активирует уже запущенное окно, наиболее похожее по заголовку на запрос.
#[cfg(windows)]
pub fn switch_to_window(query: &str) -> AppResult<String> {
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    let candidates = {
        let vec: Mutex<Vec<(HWND, String)>> = Mutex::new(Vec::new());
        unsafe {
            let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&vec as *const _ as isize));
        }
        vec.into_inner().unwrap()
    };

    if candidates.is_empty() {
        return Err(AppError::Config("не найдено видимых окон".into()));
    }

    crate::vlog!(
        "voice command window candidates: {:?}",
        candidates
            .iter()
            .map(|(_, t)| t.clone())
            .collect::<Vec<_>>()
    );

    let query_norm = normalize(query);

    // Сначала пробуем точное/подстроковое совпадение по всем словам запроса.
    if let Some(title) = candidates
        .iter()
        .find(|(_, t)| {
            let t_norm = normalize(t);
            query_norm
                .split_whitespace()
                .all(|word| t_norm.contains(word))
        })
        .map(|(_, t)| t.clone())
    {
        return activate(&title, candidates);
    }

    // Fallback: наименьшее расстояние Левенштейна между запросом и заголовком.
    crate::vlog!("voice command: using fuzzy match for '{}'", query_norm);
    let query_chars: Vec<char> = query_norm.chars().collect();
    let best = candidates
        .iter()
        .min_by_key(|(_, t)| {
            let t_chars: Vec<char> = normalize(t).chars().collect();
            levenshtein_chars(&query_chars, &t_chars)
        })
        .cloned()
        .ok_or_else(|| AppError::Config("не удалось выбрать окно".into()))?;

    activate(&best.1, candidates)
}

#[cfg(windows)]
fn activate(title: &str, candidates: Vec<(HWND, String)>) -> AppResult<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let hwnd = candidates
        .into_iter()
        .find(|(_, t)| t == title)
        .map(|(h, _)| h)
        .ok_or_else(|| AppError::Config(format!("окно «{title}» не найдено")))?;

    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }

    Ok(title.to_string())
}

#[cfg(windows)]
unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    // Пропускаем окна текущего процесса (Fono).
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == GetCurrentProcessId() {
        return BOOL(1);
    }

    let mut buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut buf);
    if len == 0 {
        return BOOL(1);
    }
    let title = String::from_utf16_lossy(&buf[..len as usize])
        .trim()
        .to_string();
    if title.is_empty() {
        return BOOL(1);
    }

    let vec = &*(lparam.0 as *const Mutex<Vec<(HWND, String)>>);
    if let Ok(mut v) = vec.lock() {
        v.push((hwnd, title));
    }

    BOOL(1)
}

#[cfg(not(windows))]
pub fn switch_to_window(_query: &str) -> AppResult<String> {
    Err(AppError::Config(
        "переключение окон поддерживается только на Windows".into(),
    ))
}

/// Запускает приложение из настроенного списка по имени/алиасу.
pub fn launch_application(query: &str, launch_apps: &[LaunchApp]) -> AppResult<String> {
    let query_norm = normalize(query);

    let app = launch_apps
        .iter()
        .find(|a| {
            let name_norm = normalize(&a.name);
            if name_norm.contains(&query_norm) || query_norm.contains(&name_norm) {
                return true;
            }
            a.aliases.iter().any(|alias| {
                let alias_norm = normalize(alias);
                alias_norm.contains(&query_norm) || query_norm.contains(&alias_norm)
            })
        })
        .ok_or_else(|| {
            AppError::Config(format!(
                "приложение «{query}» не найдено в списке запуска. Добавьте его в настройках."
            ))
        })?;

    if !std::path::Path::new(&app.exe_path).exists() {
        return Err(AppError::Config(format!(
            "файл не найден: {}",
            app.exe_path
        )));
    }

    std::process::Command::new(&app.exe_path)
        .spawn()
        .map_err(|e| AppError::Io(e))?;

    Ok(app.name.clone())
}

#[cfg(windows)]
fn send_media_key(vk: u16) -> AppResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };

    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [down, up];
    unsafe {
        let cbsize = std::mem::size_of::<INPUT>() as i32;
        let sent = SendInput(&inputs, cbsize);
        if sent == 0 {
            return Err(AppError::Injection("SendInput(media key) вернул 0".into()));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn send_media_key(_vk: u16) -> AppResult<()> {
    Err(AppError::Config(
        "медиа-клавиши поддерживаются только на Windows".into(),
    ))
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != ' ', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_prefixes<'a>(s: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    for prefix in prefixes {
        if let Some(rest) = s.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }
    None
}

fn levenshtein_chars(a: &[char], b: &[char]) -> usize {
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev = (0..=m).collect::<Vec<_>>();
    let mut curr = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}
