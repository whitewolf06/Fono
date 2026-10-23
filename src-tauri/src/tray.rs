//! Native tray presentation reads current state after the UI dispatch hop.
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItem, MenuItemBuilder},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Listener, Manager, Wry,
};

use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
    stt::SttReadiness,
    types::{PipelineState, UiLocale},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ready,
    WakeListening,
    Paused,
    Recording,
    Transcribing,
    Processing,
    AwaitingAction,
    Injecting,
    Loading,
    Unloaded,
    Error,
}

impl Status {
    fn tooltip(self, locale: UiLocale) -> &'static str {
        if locale == UiLocale::En {
            return match self {
                Self::Ready => "Fono — ready to dictate",
                Self::WakeListening => "Fono — listening for Privet Lira (microphone on)",
                Self::Paused => "Fono — dictation paused",
                Self::Recording => "Fono — recording speech",
                Self::Transcribing => "Fono — recognizing speech",
                Self::Processing => "Fono — processing text",
                Self::AwaitingAction => "Fono — text is waiting for your action",
                Self::Injecting => "Fono — inserting text",
                Self::Loading => "Fono — loading recognition model",
                Self::Unloaded => "Fono — recognition model is not loaded",
                Self::Error => "Fono — dictation error; open the application",
            };
        }
        match self {
            Self::Ready => "Fono — готов к диктовке",
            Self::WakeListening => "Fono — слушаю «Привет, Лира» (микрофон включён)",
            Self::Paused => "Fono — диктовка приостановлена",
            Self::Recording => "Fono — запись голоса",
            Self::Transcribing => "Fono — распознавание речи",
            Self::Processing => "Fono — обработка текста",
            Self::AwaitingAction => "Fono — текст ждёт вашего действия",
            Self::Injecting => "Fono — вставка текста",
            Self::Loading => "Fono — загрузка модели распознавания",
            Self::Unloaded => "Fono — модель распознавания не загружена",
            Self::Error => "Fono — ошибка диктовки; откройте приложение",
        }
    }

    fn color(self) -> [u8; 4] {
        match self {
            Self::Recording => [224, 67, 68, 255],
            Self::Paused | Self::AwaitingAction => [234, 178, 60, 255],
            Self::Error => [235, 79, 73, 255],
            Self::Ready | Self::WakeListening => [54, 184, 136, 255],
            Self::Unloaded => [151, 160, 174, 255],
            _ => [91, 158, 246, 255],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Appearance {
    locale: UiLocale,
    status: Status,
    paused: bool,
    lira_enabled: bool,
    #[cfg(feature = "wake-word")]
    wake_enabled: bool,
}

fn status(pipeline: PipelineState, paused: bool, readiness: &SttReadiness) -> Status {
    // A requested pause does not pretend that capture/processing has already
    // ended. The icon follows its actual state until cancellation completes.
    match pipeline {
        PipelineState::Listening => Status::Recording,
        PipelineState::Transcribing => Status::Transcribing,
        PipelineState::Processing => Status::Processing,
        PipelineState::AwaitingAction => Status::AwaitingAction,
        PipelineState::Injecting => Status::Injecting,
        PipelineState::Error => Status::Error,
        PipelineState::Idle if paused => Status::Paused,
        PipelineState::Idle => match readiness {
            SttReadiness::Unloaded => Status::Unloaded,
            SttReadiness::Loading => Status::Loading,
            SttReadiness::Ready { .. } => Status::Ready,
            SttReadiness::Failed { .. } => Status::Error,
        },
    }
}

fn lira_label(enabled: bool, locale: UiLocale) -> &'static str {
    match (enabled, locale == UiLocale::En) {
        (true, true) => "Turn Privet Lira off",
        (false, true) => "Enable Privet Lira",
        (true, false) => "Выключить «Привет, Лира»",
        (false, false) => "Включить «Привет, Лира»",
    }
}

fn pause_label(paused: bool, locale: UiLocale) -> &'static str {
    if locale == UiLocale::En {
        return if paused {
            "Resume dictation"
        } else {
            "Pause dictation"
        };
    }
    if paused {
        "Продолжить диктовку"
    } else {
        "Приостановить диктовку"
    }
}

#[cfg(feature = "wake-word")]
fn wake_label(enabled: bool, locale: UiLocale) -> &'static str {
    if locale == UiLocale::En {
        return if enabled {
            "Disable voice wake-up"
        } else {
            "Enable voice wake-up"
        };
    }
    if enabled {
        "Выключить голосовое пробуждение"
    } else {
        "Включить голосовое пробуждение"
    }
}

struct TrayPresentation {
    show: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    icon: TrayIcon<Wry>,
    base: Image<'static>,
    pause: MenuItem<Wry>,
    lira: Option<MenuItem<Wry>>,
    #[cfg(feature = "wake-word")]
    wake: Option<MenuItem<Wry>>,
    last: Mutex<Option<Appearance>>,
    queued: AtomicBool,
}

pub(crate) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let paused = app.state::<AppState>().is_dictation_paused();
    let locale = app.state::<AppState>().settings().ui_language.resolve();
    let show = MenuItemBuilder::with_id(
        "show",
        if locale == UiLocale::En {
            "Open Fono"
        } else {
            "Открыть Fono"
        },
    )
    .build(app)?;
    let pause = MenuItemBuilder::with_id("pause", pause_label(paused, locale)).build(app)?;
    let quit = MenuItemBuilder::with_id(
        "quit",
        if locale == UiLocale::En {
            "Quit"
        } else {
            "Выход"
        },
    )
    .build(app)?;
    let menu_builder = MenuBuilder::new(app).item(&show).separator().item(&pause);
    let lira = if crate::application::wake_lira::available() {
        Some(MenuItemBuilder::with_id("lira_live", lira_label(false, locale)).build(app)?)
    } else {
        None
    };
    let menu_builder = if let Some(item) = &lira {
        menu_builder.separator().item(item)
    } else {
        menu_builder
    };
    #[cfg(feature = "wake-word")]
    let wake = if crate::types::WAKE_WORD_AVAILABLE {
        Some(
            MenuItemBuilder::with_id(
                "wake_word",
                wake_label(app.state::<AppState>().settings().wake_word_enabled, locale),
            )
            .build(app)?,
        )
    } else {
        None
    };
    #[cfg(feature = "wake-word")]
    let menu_builder = if let Some(item) = &wake {
        menu_builder.separator().item(item)
    } else {
        menu_builder
    };
    let menu = menu_builder.separator().item(&quit).build()?;
    let base = app
        .default_window_icon()
        .ok_or_else(|| AppError::Config("Не удалось загрузить значок Fono для трея".into()))?;
    let base = Image::new_owned(base.rgba().to_vec(), base.width(), base.height());
    // tauri.conf.json normally creates this icon. Reuse it so setup cannot
    // leave a second, static tray icon behind.
    let icon = match app.tray_by_id("main") {
        Some(icon) => icon,
        None => TrayIconBuilder::with_id("main")
            .icon(base.clone())
            .build(app)?,
    };
    icon.set_menu(Some(menu))?;
    icon.on_menu_event(menu_event);
    app.manage(TrayPresentation {
        show,
        quit,
        icon,
        base,
        pause,
        lira,
        #[cfg(feature = "wake-word")]
        wake,
        last: Mutex::new(None),
        queued: AtomicBool::new(false),
    });
    let handle = app.handle().clone();
    app.listen(crate::events::EVENT_CHANNEL_V1, move |event| {
        #[derive(serde::Deserialize)]
        struct Kind<'a> {
            kind: &'a str,
        }
        if serde_json::from_str::<Kind<'_>>(event.payload()).is_ok_and(|event| {
            matches!(event.kind, "pipeline_state" | "stt_readiness" | "settings")
        }) {
            request_refresh(&handle);
        }
    });
    // Setup runs on the UI thread and uses the actual initial pause/readiness.
    refresh_on_ui(app.handle())?;
    Ok(())
}

fn menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        "show" => {
            if let Some(window) = app.get_webview_window("settings") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "pause" => {
            let paused = app.state::<AppState>().toggle_dictation_paused();
            request_refresh(app);
            if paused {
                // Bind this click to its operation before queueing. A later
                // Resume/start must never become this Pause request's target.
                let target = app
                    .state::<Pipeline>()
                    .current_operation()
                    .map(|operation| operation.id);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if !app.state::<AppState>().is_dictation_paused() {
                        return;
                    }
                    let Some(target) = target else {
                        return;
                    };
                    if let Err(error) =
                        crate::ipc::dictation::cancel_dictation(app.clone(), Some(target)).await
                    {
                        if !matches!(error, AppError::Cancelled(_)) {
                            tracing::warn!(%error, "pause cancellation failed");
                            crate::events::emit_error(
                                &app,
                                crate::events::ErrorCodeV1::Audio,
                                error.user_message(),
                                Some(target),
                            );
                        }
                    }
                    request_refresh(&app);
                });
            }
        }
        #[cfg(feature = "wake-word")]
        "wake_word" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let enabled = app.state::<AppState>().settings().wake_word_enabled;
                let result = if enabled {
                    crate::ipc::wake::disable_wake_word(app.clone()).await
                } else {
                    crate::ipc::wake::enable_wake_word(app.clone()).await
                };
                if let Err(error) = result {
                    tracing::warn!(%error, "tray wake toggle failed");
                    crate::events::emit_error(
                        &app,
                        crate::events::ErrorCodeV1::Wake,
                        error.user_message(),
                        None,
                    );
                }
                request_refresh(&app);
            });
        }
        "lira_live" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let view = app
                    .state::<crate::application::wake_lira::Runtime>()
                    .snapshot();
                let result = if view.active() {
                    if let Some(id) = view.session_id {
                        crate::application::wake_lira::stop(app.clone(), id)
                            .await
                            .map(|_| ())
                    } else {
                        Ok(())
                    }
                } else {
                    app.state::<crate::application::wake_lira::Runtime>()
                        .start(&app)
                        .map(|_| ())
                };
                if let Err(error) = result {
                    crate::events::emit_error(
                        &app,
                        crate::events::ErrorCodeV1::Wake,
                        error.user_message(),
                        None,
                    );
                }
                request_refresh(&app);
            });
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

pub(crate) fn request_refresh(app: &AppHandle) {
    let Some(runtime) = app.try_state::<TrayPresentation>() else {
        return;
    };
    if runtime.queued.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    // Backend publishers may hold an operation fence. Always hop asynchronously
    // before entering native UI; read state only when that UI task executes.
    tauri::async_runtime::spawn(async move {
        let ui_app = app.clone();
        if app
            .run_on_main_thread(move || {
                if let Some(runtime) = ui_app.try_state::<TrayPresentation>() {
                    runtime.queued.store(false, Ordering::Release);
                }
                if let Err(error) = refresh_on_ui(&ui_app) {
                    tracing::warn!(%error, "could not refresh tray");
                }
            })
            .is_err()
        {
            if let Some(runtime) = app.try_state::<TrayPresentation>() {
                runtime.queued.store(false, Ordering::Release);
            }
            tracing::warn!("could not dispatch tray refresh");
        }
    });
}

fn refresh_on_ui(app: &AppHandle) -> AppResult<()> {
    let Some(runtime) = app.try_state::<TrayPresentation>() else {
        return Ok(());
    };
    let state = app.state::<AppState>();
    let paused = state.is_dictation_paused();
    let lira = app
        .state::<crate::application::wake_lira::Runtime>()
        .snapshot();
    let pipeline = app.state::<Pipeline>();
    let base_status = status(pipeline.state(), paused, &pipeline.stt().readiness());
    let live_status = if pipeline.current_operation().is_none() && !paused {
        use crate::application::wake_lira::Phase;
        match lira.phase {
            Phase::Listening => Status::WakeListening,
            Phase::Loading | Phase::Stopping => Status::Loading,
            Phase::Paused => Status::Paused,
            Phase::Error => Status::Error,
            Phase::Off => base_status,
        }
    } else {
        base_status
    };
    let appearance = Appearance {
        locale: state.settings().ui_language.resolve(),
        status: live_status,
        paused,
        lira_enabled: lira.active(),
        #[cfg(feature = "wake-word")]
        wake_enabled: state.settings().wake_word_enabled,
    };
    if runtime.last.lock().as_ref() == Some(&appearance) {
        return Ok(());
    }
    runtime
        .pause
        .set_text(pause_label(paused, appearance.locale))?;
    runtime
        .show
        .set_text(if appearance.locale == UiLocale::En {
            "Open Fono"
        } else {
            "Открыть Fono"
        })?;
    runtime
        .quit
        .set_text(if appearance.locale == UiLocale::En {
            "Quit"
        } else {
            "Выход"
        })?;
    #[cfg(feature = "wake-word")]
    if let Some(wake) = &runtime.wake {
        wake.set_text(wake_label(appearance.wake_enabled, appearance.locale))?;
    }
    if let Some(lira) = &runtime.lira {
        lira.set_text(lira_label(appearance.lira_enabled, appearance.locale))?;
    }
    runtime
        .icon
        .set_tooltip(Some(appearance.status.tooltip(appearance.locale)))?;
    runtime
        .icon
        .set_icon(Some(status_icon(&runtime.base, appearance.status)?))?;
    *runtime.last.lock() = Some(appearance);
    Ok(())
}

fn status_icon(base: &Image<'_>, status: Status) -> AppResult<Image<'static>> {
    let (width, height) = (base.width() as usize, base.height() as usize);
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(base.rgba().len()) {
        return Err(AppError::Internal("Некорректный размер значка трея".into()));
    }
    let mut pixels = base.rgba().to_vec();
    let diameter = width.min(height).div_ceil(2);
    let radius = diameter as f64 / 2.0;
    let center = (width as f64 - radius, height as f64 - radius);
    for y in height - diameter..height {
        for x in width - diameter..width {
            let (dx, dy) = (x as f64 + 0.5 - center.0, y as f64 + 0.5 - center.1);
            let distance = dx * dx + dy * dy;
            if distance > radius * radius {
                continue;
            }
            let mut color = if distance >= (radius * 0.76).powi(2) {
                [255, 255, 255, 255]
            } else {
                status.color()
            };
            if status == Status::Paused
                && dy.abs() < radius * 0.42
                && dx.abs() > radius * 0.12
                && dx.abs() < radius * 0.40
            {
                color = [30, 35, 42, 255];
            } else if status == Status::Error
                && dx.abs() < radius * 0.13
                && dy.abs() < radius * 0.50
            {
                color = [255, 255, 255, 255];
            }
            let offset = (y * width + x) * 4;
            pixels[offset..offset + 4].copy_from_slice(&color);
        }
    }
    Ok(Image::new_owned(pixels, base.width(), base.height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_tray_and_menu_follow_pause_and_real_model_readiness() {
        assert_eq!(pause_label(true, UiLocale::Ru), "Продолжить диктовку");
        assert_eq!(pause_label(false, UiLocale::Ru), "Приостановить диктовку");
        assert_eq!(pause_label(true, UiLocale::En), "Resume dictation");
        assert_eq!(pause_label(false, UiLocale::En), "Pause dictation");
        assert_eq!(
            Status::Recording.tooltip(UiLocale::En),
            "Fono — recording speech"
        );
        assert_eq!(
            status(PipelineState::Idle, true, &SttReadiness::Unloaded),
            Status::Paused
        );
        assert_eq!(
            status(PipelineState::Idle, false, &SttReadiness::Unloaded),
            Status::Unloaded
        );
        assert_eq!(
            status(PipelineState::Idle, false, &SttReadiness::Loading),
            Status::Loading
        );
        assert_eq!(
            status(
                PipelineState::Idle,
                false,
                &SttReadiness::Ready {
                    device: "CPU".into()
                }
            ),
            Status::Ready
        );
        assert_eq!(
            status(
                PipelineState::Idle,
                false,
                &SttReadiness::Failed {
                    message: "fixture".into()
                }
            ),
            Status::Error
        );
    }

    #[test]
    fn requested_pause_keeps_actual_capture_visible_until_it_really_finishes() {
        assert_eq!(
            status(PipelineState::Listening, true, &SttReadiness::Loading),
            Status::Recording
        );
        assert_eq!(
            status(PipelineState::Transcribing, true, &SttReadiness::Unloaded),
            Status::Transcribing
        );
        assert_eq!(
            status(
                PipelineState::AwaitingAction,
                false,
                &SttReadiness::Unloaded
            ),
            Status::AwaitingAction
        );
        assert_eq!(
            status(PipelineState::Injecting, false, &SttReadiness::Unloaded),
            Status::Injecting
        );
    }

    #[test]
    fn badges_change_actual_pixels_preserve_brand_and_reject_invalid_buffers() {
        let base = Image::new_owned(vec![20; 32 * 32 * 4], 32, 32);
        let recording = status_icon(&base, Status::Recording).unwrap();
        let paused = status_icon(&base, Status::Paused).unwrap();
        let ready = status_icon(&base, Status::Ready).unwrap();
        assert_ne!(recording.rgba(), paused.rgba());
        assert_ne!(recording.rgba(), ready.rgba());
        assert_eq!(
            &recording.rgba()[..32 * 16 * 4],
            &base.rgba()[..32 * 16 * 4]
        );
        assert!(status_icon(&Image::new_owned(vec![], 32, 32), Status::Ready).is_err());
    }
}
