use super::{insertion, session::LiveSession};
use crate::application::{live_agreement::LiveAgreement, speech_gate::SpeechGate};
use crate::{
    error::{AppError, AppResult},
    pipeline::{self, Pipeline},
    state::AppState,
    types::PipelineState,
};
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, Manager};
mod decode;
mod terminal;
const RATE: u64 = 16_000;
const WINDOW: usize = 20 * RATE as usize;

pub(super) async fn run(app: AppHandle, session: Arc<LiveSession>, mut gate: SpeechGate) {
    let mut agreement = LiveAgreement::default();
    let result = drive(&app, &session, &mut gate, &mut agreement).await;
    if result.is_ok() {
        // A final hypothesis can append more than one bounded insertion chunk.
        insertion::drain(&app, &session).await;
    }
    terminal::complete(&app, &session, &agreement, result);
}

async fn drive(
    app: &AppHandle,
    session: &Arc<LiveSession>,
    gate: &mut SpeechGate,
    agreement: &mut LiveAgreement,
) -> AppResult<()> {
    decode::load(app, session).await?;
    let mut consumed = 0;
    let mut decoded_end = 0;
    let mut decode_from = 0;
    let mut speech = gate.accept(&[]);
    let mut last_decode = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let mut last_audio = std::time::Instant::now();
    let mut observed_end = 0;
    loop {
        if session.cancellation.is_cancelled() {
            return Err(AppError::Cancelled("Диктовка отменена".into()));
        }
        let pipeline = app.state::<Pipeline>();
        if !pipeline.is_operation_active(session.operation) {
            return Err(AppError::Cancelled("Диктовка заменена".into()));
        }
        let audio_end = pipeline.recording_end();
        if audio_end != observed_end {
            observed_end = audio_end;
            last_audio = std::time::Instant::now();
        }
        if pipeline.is_recording() {
            if let Some(error) = app.state::<fono_wake::AudioHub>().diagnostics().last_error {
                return Err(AppError::Audio(format!("Поток микрофона прерван: {error}")));
            }
            if last_audio.elapsed().as_secs() >= 3 {
                return Err(AppError::Audio(
                    "Микрофон перестал передавать звук. Проверьте устройство".into(),
                ));
            }
        }
        while consumed < audio_end {
            let (from, samples) = pipeline.recording_window(consumed, WINDOW);
            if from != consumed || samples.is_empty() {
                return Err(AppError::Audio(
                    "Часть аудиопотока потеряна; доступный текст сохранён".into(),
                ));
            }
            consumed += samples.len() as u64;
            speech = gate.accept(&samples);
        }
        let reason = if pipeline.recording_limit_reached() {
            Some("Распознавание отстаёт. Запись остановлена; сохранённый звук будет обработан")
        } else if session.started.elapsed().as_secs() >= 30 * 60 {
            Some("Достигнут предел 30 минут; завершаем диктовку")
        } else if !speech.has_speech && session.started.elapsed().as_secs() >= 5 {
            Some("Речь не обнаружена. Проверьте микрофон и повторите диктовку")
        } else if speech.has_speech && consumed.saturating_sub(speech.last_speech) >= 30 * RATE {
            Some("Запись завершена после 30 секунд тишины")
        } else {
            None
        };
        if let Some(reason) = reason.filter(|_| !session.stopping()) {
            session.data.lock().snapshot.warning = Some(reason.into());
            session.stop.store(true, Ordering::Release);
            pipeline.stop_capture_for(session.operation)?;
        }
        if session.stopping() && !pipeline.is_recording() {
            session.data.lock().snapshot.phase = "draining".into();
            pipeline::set_state_for_operation(
                app,
                app.state::<AppState>().inner(),
                &pipeline,
                session.operation,
                PipelineState::Transcribing,
                fono_core::TerminalReason::Completed,
            );
        }
        let needs_decode = speech.has_speech && audio_end > decoded_end;
        let settling = speech.has_speech && decoded_end == audio_end && !agreement.draft.is_empty();
        if (needs_decode || settling)
            && (session.stopping() || last_decode.elapsed().as_millis() >= 1_000)
        {
            decode_from = decode_from.max(pipeline.recording_start());
            let (from, samples) = pipeline.recording_window(decode_from, WINDOW);
            let end = from + samples.len() as u64;
            if samples.is_empty() {
                break;
            }
            let quiet_endpoint = !speech.speaking
                && audio_end.saturating_sub(speech.last_speech) >= RATE * 7 / 10
                && end >= speech.last_speech;
            let final_tail =
                session.stopping() && !pipeline.is_recording() && end == pipeline.recording_end();
            let bounded_endpoint = samples.len() == WINDOW;
            let mut hypothesis = decode::window(app, session, samples, from).await?;
            if bounded_endpoint && !final_tail {
                // Keep a quiet boundary margin. The next overlapping window owns it.
                hypothesis
                    .words
                    .retain(|word| word.end_sample <= end.saturating_sub(RATE / 2));
            }
            if quiet_endpoint && !final_tail {
                // A new syllable can precede VAD's speech confirmation. Keep
                // that fresh edge for the next utterance instead of finalizing it.
                hypothesis
                    .words
                    .retain(|word| word.end_sample <= speech.last_speech);
            }
            let appended = agreement.accept(
                &hypothesis,
                end,
                final_tail || quiet_endpoint || bounded_endpoint,
            );
            decoded_end = end;
            last_decode = std::time::Instant::now();
            {
                let mut data = session.data.lock();
                data.decode_seconds += hypothesis.transcribe_secs;
                data.snapshot.committed_text = agreement.committed.clone();
                data.snapshot.draft_text = agreement.draft.clone();
                data.snapshot.pending_text.push_str(&appended);
                data.snapshot.lag_ms =
                    speech.last_speech.saturating_sub(agreement.committed_end) * 1_000 / RATE;
            }
            decode_from = crate::application::live_windows::next_start(decode_from, agreement.committed_end,
                (quiet_endpoint && !final_tail).then_some(speech.last_speech), end, bounded_endpoint && !final_tail)
                .ok_or_else(|| AppError::Stt("Модель не подтвердила слова в длинном фрагменте. Диктовка остановлена; доступный текст сохранён".into()))?;
            pipeline.discard_audio_before(decode_from.min(consumed));
            insertion::flush(app, session).await;
            session.emit(app);
            if final_tail && end == pipeline.recording_end() {
                break;
            }
            if session.stopping() {
                continue;
            }
        }
        if session.stopping()
            && !pipeline.is_recording()
            && consumed == pipeline.recording_end()
            && (!speech.has_speech || (decoded_end >= audio_end && agreement.draft.is_empty()))
        {
            break;
        }
        insertion::flush(app, session).await;
        session.data.lock().snapshot.audio_level = (pipeline.current_level() * 8.0).clamp(0.0, 1.0);
        session.emit(app);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(())
}
