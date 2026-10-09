use super::super::session::LiveSession;
use crate::{
    application::{dictation_result, live_agreement::LiveAgreement},
    error::AppResult,
    pipeline::{self, Pipeline},
    state::AppState,
    types::{PipelineState, Transcript},
};
use fono_core::TerminalReason;
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, Manager};

pub(super) fn complete(
    app: &AppHandle,
    session: &Arc<LiveSession>,
    agreement: &LiveAgreement,
    result: AppResult<()>,
) {
    // Seal with Cancel before publication, keeping one outcome during persistence.
    let cancelled = session.seal_outcome();
    let reason = if cancelled {
        TerminalReason::Cancelled
    } else if result.is_err() {
        TerminalReason::Failed
    } else {
        TerminalReason::Completed
    };
    let mut text = agreement.committed.clone();
    if (cancelled || result.is_err()) && !agreement.draft.is_empty() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&agreement.draft);
    }
    let transcript = Transcript {
        text,
        detected_language: Some(session.settings.language.clone()),
        transcribe_secs: Some(session.data.lock().decode_seconds),
        audio_secs: Some(app.state::<Pipeline>().recording_end() as f32 / 16_000.0),
        device: Some(app.state::<Pipeline>().stt().device()),
    };
    if super::super::owns(app, session) {
        {
            let mut data = session.data.lock();
            data.snapshot.phase = if cancelled {
                "cancelled"
            } else if result.is_err() {
                "error"
            } else {
                "done"
            }
            .into();
            data.snapshot.audio_level = 0.0;
            if let Err(error) = &result {
                if !cancelled {
                    data.snapshot.warning = Some(error.to_string());
                }
            }
        }
        // Even disabled history retains the latest partial result in memory.
        dictation_result::publish(
            app,
            &session.operation.to_string(),
            &transcript,
            &transcript.text,
        );
        if !cancelled && result.is_ok() {
            dictation_result::archive(
                app,
                &session.settings,
                crate::history::next_id(),
                &transcript,
                &transcript.text,
                session.operation,
            );
        }
        let pipeline = app.state::<Pipeline>();
        let _ = pipeline.stop_recording_for(session.operation);
        pipeline::set_state_for_operation(
            app,
            app.state::<AppState>().inner(),
            &pipeline,
            session.operation,
            PipelineState::Idle,
            reason,
        );
        app.state::<fono_wake::WakeWordHandle>().resume();
        if let Err(error) = &result {
            if !cancelled {
                crate::events::emit_error(
                    app,
                    crate::events::ErrorCodeV1::Stt,
                    error.to_string(),
                    Some(session.operation),
                );
            }
        }
        session.emit(app);
    }
    session.finished.store(true, Ordering::Release);
    session.completion.send_replace(Some(if cancelled {
        Ok(transcript)
    } else {
        result
            .map(|_| transcript)
            .map_err(|error| error.to_string())
    }));
}
