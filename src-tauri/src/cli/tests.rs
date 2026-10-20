use super::*;
use fono_core::OperationSource;

fn operation(id: u64, phase: OperationPhase) -> OperationSnapshot {
    OperationSnapshot {
        id,
        phase,
        source: OperationSource::Hotkey,
        confirmed: false,
    }
}

#[test]
fn idle_start_toggle_and_repeated_stop_cancel_have_explicit_semantics() {
    assert_eq!(plan(Command::Start, None).unwrap(), Plan::Start);
    assert_eq!(plan(Command::Toggle, None).unwrap(), Plan::Start);
    assert_eq!(plan(Command::Stop, None).unwrap(), Plan::None);
    assert_eq!(plan(Command::Cancel, None).unwrap(), Plan::None);
    assert_eq!(plan(Command::Show, None).unwrap(), Plan::Show);
    let preparing = operation(7, OperationPhase::Recording);
    assert_eq!(plan(Command::Start, Some(&preparing)).unwrap(), Plan::None);
    assert_eq!(
        plan(Command::Toggle, Some(&preparing)).unwrap(),
        Plan::Stop(7)
    );
    assert_eq!(
        plan(Command::Stop, Some(&preparing)).unwrap(),
        Plan::Stop(7)
    );
    assert_eq!(
        plan(Command::Cancel, Some(&preparing)).unwrap(),
        Plan::Cancel(7)
    );
}

#[test]
fn delayed_stop_cancel_are_frozen_to_original_identity_and_processing_never_starts_again() {
    let old = operation(7, OperationPhase::Recording);
    let stop = plan(Command::Stop, Some(&old)).unwrap();
    let cancel = plan(Command::Cancel, Some(&old)).unwrap();
    let next = operation(8, OperationPhase::Recording);
    assert_eq!(stop, Plan::Stop(7));
    assert_eq!(cancel, Plan::Cancel(7));
    assert_eq!(plan(Command::Stop, Some(&next)).unwrap(), Plan::Stop(8));
    for phase in [
        OperationPhase::Transcribing,
        OperationPhase::Processing,
        OperationPhase::AwaitingAction,
        OperationPhase::Injecting,
    ] {
        let busy = operation(8, phase);
        assert!(plan(Command::Start, Some(&busy)).is_err());
        assert!(plan(Command::Toggle, Some(&busy)).is_err());
        assert_eq!(plan(Command::Stop, Some(&busy)).unwrap(), Plan::None);
        assert_eq!(plan(Command::Cancel, Some(&busy)).unwrap(), Plan::Cancel(8));
    }
}

#[test]
fn duplicate_stop_coalesces_until_real_work_finishes_then_retry_is_allowed() {
    let claims = Claims::default();
    let first = Claim::take(&claims, Work::Stop, 7).unwrap();
    assert!(Claim::take(&claims, Work::Stop, 7).is_none());
    // Cancel can overtake a waiting Stop; another identity is independent.
    let cancel = Claim::take(&claims, Work::Cancel, 7).unwrap();
    assert!(Claim::take(&claims, Work::Cancel, 7).is_none());
    assert!(Claim::take(&claims, Work::Stop, 8).is_some());
    drop(cancel);
    drop(first);
    assert!(Claim::take(&claims, Work::Stop, 7).is_some());
}

#[test]
fn record_commands_start_without_focusing_settings_or_changing_overlay_policy() {
    use tauri::utils::config::WindowConfig;
    for command in [
        Command::Start,
        Command::Stop,
        Command::Toggle,
        Command::Cancel,
        Command::Show,
    ] {
        let mut windows = vec![
            WindowConfig {
                label: "settings".into(),
                visible: true,
                focus: true,
                ..Default::default()
            },
            WindowConfig {
                label: "overlay".into(),
                visible: false,
                focus: false,
                focusable: false,
                ..Default::default()
            },
        ];
        configure_windows(&mut windows, command);
        assert_eq!(windows[0].visible, command == Command::Show);
        assert_eq!(windows[0].focus, command == Command::Show);
        assert!(!windows[1].visible);
        assert!(!windows[1].focus);
        assert!(!windows[1].focusable);
    }
}

#[test]
fn stop_never_reinterprets_diagnostic_audio_as_ordinary_dictation() {
    let mut diagnostic = operation(7, OperationPhase::Recording);
    diagnostic.source = OperationSource::Diagnostics;
    for command in [Command::Start, Command::Stop, Command::Toggle] {
        assert!(plan(command, Some(&diagnostic)).is_err());
    }
    assert_eq!(
        plan(Command::Cancel, Some(&diagnostic)).unwrap(),
        Plan::Cancel(7)
    );
    assert_eq!(plan(Command::Show, Some(&diagnostic)).unwrap(), Plan::Show);
}
