use crate::{
    operation::{OperationPhase, OperationSource, TerminalReason},
    pipeline::Pipeline,
    types::{AiMode, HotkeyMode, PipelineState, Settings, TextPreset, TranslationLanguage},
};
use std::sync::{mpsc, Arc};

fn recording() -> (Pipeline, u64) {
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    *pipeline.recording.lock() = true;
    assert!(pipeline.set_session_settings_for(
        operation.id,
        Settings {
            ai_mode: AiMode::Clean,
            ..Settings::default()
        },
    ));
    (pipeline, operation.id)
}

#[test]
fn stop_freezes_selection_before_audio_drain_changes_the_public_phase() {
    let (pipeline, id) = recording();
    let (_, _, frozen) = pipeline
        .freeze_session_for_completion(id, Settings::default)
        .unwrap();
    assert!(pipeline.is_recording());
    assert_eq!(
        pipeline.current_operation().unwrap().phase,
        OperationPhase::Recording
    );
    assert!(pipeline
        .with_processing_choice::<()>(Some(id), |_, _| panic!("late choice persisted"))
        .is_err());
    assert_eq!(frozen.processing_preset, None);
}

#[test]
fn failed_stop_restores_ownership_only_for_the_exact_editable_capture() {
    let (pipeline, id) = recording();
    let mut restored = 0;
    assert!(pipeline.while_editable_recording(id, || restored += 1));
    assert_eq!(restored, 1);
    assert!(!pipeline.while_editable_recording(id + 1, || panic!("wrong capture")));
    pipeline
        .freeze_session_for_completion(id, Settings::default)
        .unwrap();
    assert!(!pipeline.while_editable_recording(id, || panic!("frozen capture")));
    pipeline.cancel_for(id).unwrap();
    let next = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    assert!(!pipeline.while_editable_recording(id, || panic!("successor capture")));
    assert!(pipeline.while_editable_recording(next.id, || restored += 1));
}

#[test]
fn selection_commits_before_concurrent_stop_can_snapshot_it() {
    let (pipeline, id) = recording();
    let pipeline = Arc::new(pipeline);
    let (persisting_tx, persisting_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    std::thread::scope(|threads| {
        let choice_pipeline = pipeline.clone();
        let choice = threads.spawn(move || {
            choice_pipeline
                .with_processing_choice(Some(id), |_, settings| {
                    persisting_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    let settings = settings.unwrap();
                    settings.processing_preset = Some(TextPreset::Formal);
                    settings.processing_target_language = Some(TranslationLanguage::En);
                    Ok(())
                })
                .unwrap();
        });
        persisting_rx.recv().unwrap();
        let stop_pipeline = pipeline.clone();
        let stop = threads.spawn(move || {
            stop_pipeline
                .freeze_session_for_completion(id, Settings::default)
                .unwrap()
                .2
        });
        release_tx.send(()).unwrap();
        choice.join().unwrap();
        let settings = stop.join().unwrap();
        assert_eq!(settings.processing_preset, Some(TextPreset::Formal));
        assert_eq!(
            settings.processing_target_language,
            Some(TranslationLanguage::En)
        );
    });
}

#[test]
fn cancelled_session_cannot_change_successors_choice_or_idle_defaults() {
    let (pipeline, old) = recording();
    pipeline.cancel_for(old).unwrap();
    let next = pipeline.operations.start(OperationSource::Ui).unwrap();
    pipeline.set_session_settings_for(next.id, Settings::default());
    assert!(pipeline
        .with_processing_choice::<()>(Some(old), |_, _| panic!("stale choice persisted"))
        .is_err());
    assert!(pipeline
        .with_processing_choice::<()>(None, |_, _| panic!("active default persisted"))
        .is_err());
    assert_eq!(
        pipeline
            .session_settings_for(next.id)
            .unwrap()
            .processing_preset,
        None
    );
}

#[test]
fn waiting_action_allows_choice_but_processing_and_insertion_reject_it() {
    let (pipeline, id) = recording();
    pipeline
        .freeze_session_for_completion(id, Settings::default)
        .unwrap();
    *pipeline.recording.lock() = false;
    pipeline
        .sync_operation_state_for(id, PipelineState::Transcribing, TerminalReason::Completed)
        .unwrap();
    assert!(pipeline
        .with_processing_choice(Some(id), |_, _| Ok(()))
        .is_err());
    pipeline
        .sync_operation_state_for(id, PipelineState::AwaitingAction, TerminalReason::Completed)
        .unwrap();
    pipeline
        .with_processing_choice(Some(id), |phase, _| {
            assert_eq!(phase, Some(OperationPhase::AwaitingAction));
            Ok(())
        })
        .unwrap();
    pipeline
        .sync_operation_state_for(id, PipelineState::Processing, TerminalReason::Completed)
        .unwrap();
    assert!(pipeline
        .with_processing_choice(Some(id), |_, _| Ok(()))
        .is_err());
    pipeline
        .sync_operation_state_for(id, PipelineState::Injecting, TerminalReason::Completed)
        .unwrap();
    assert!(pipeline
        .with_processing_choice(Some(id), |_, _| Ok(()))
        .is_err());
}

#[test]
fn idle_defaults_cannot_overlap_a_capture_registration() {
    let pipeline = Pipeline::new();
    pipeline
        .with_processing_choice(None, |phase, session| {
            assert_eq!(phase, None);
            assert!(session.is_none());
            Ok(())
        })
        .unwrap();
    let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
    assert!(pipeline
        .with_processing_choice::<()>(None, |_, _| panic!("busy idle write"))
        .is_err());
    pipeline.cancel_for(operation.id).unwrap();
}

#[test]
fn ui_hold_and_toggle_all_finish_with_the_accepted_session_selection() {
    for source in [OperationSource::Ui, OperationSource::Hotkey] {
        for mode in [HotkeyMode::Hold, HotkeyMode::Toggle] {
            let pipeline = Pipeline::new();
            let operation = pipeline.operations.start(source).unwrap();
            *pipeline.recording.lock() = true;
            pipeline.set_session_settings_for(
                operation.id,
                Settings {
                    hotkey_mode: mode,
                    ai_mode: AiMode::Clean,
                    ..Settings::default()
                },
            );
            pipeline
                .with_processing_choice(Some(operation.id), |_, settings| {
                    let settings = settings.unwrap();
                    settings.processing_preset = Some(TextPreset::Task);
                    settings.processing_target_language = Some(TranslationLanguage::De);
                    Ok(())
                })
                .unwrap();
            let (snapshot, _, settings) = pipeline
                .freeze_session_for_completion(operation.id, Settings::default)
                .unwrap();
            assert_eq!(snapshot.source, source);
            assert_eq!(settings.hotkey_mode, mode);
            assert_eq!(settings.processing_preset, Some(TextPreset::Task));
            assert_eq!(
                settings.processing_target_language,
                Some(TranslationLanguage::De)
            );
        }
    }
}
