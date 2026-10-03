//! Shared CPU/CUDA inference with owned reusable state and bounded PCM scratch.
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Instant;

use whisper_rs::{WhisperContext, WhisperState};

#[path = "inference_options.rs"]
mod options;

use crate::{words_from_pieces, BackendKind, TimedPiece, TimedSegment, WindowTranscript};

/// Lives on the caller's stack until synchronous `full` and all native workers
/// return. The callback does not mutate a Whisper context, leak an Arc, or unwind
/// into C. Do not use whisper-rs 0.16's leaking safe callback helper here.
struct AbortGuard<'a> {
    check: &'a (dyn Fn() -> bool + Sync),
}

unsafe extern "C" fn abort_callback(data: *mut c_void) -> bool {
    if data.is_null() {
        return true;
    }
    // SAFETY: attach passes a live, immovable stack guard for the duration of full.
    let guard = unsafe { &*data.cast::<AbortGuard<'_>>() };
    catch_unwind(AssertUnwindSafe(|| (guard.check)())).unwrap_or(true)
}

pub struct InferenceState {
    context: Arc<WhisperContext>,
    state: WhisperState,
    pcm: Vec<f32>,
}

impl InferenceState {
    pub fn new(context: Arc<WhisperContext>) -> Result<Self, String> {
        let state = context.create_state().map_err(|error| error.to_string())?;
        Ok(Self {
            context,
            state,
            pcm: Vec::new(),
        })
    }

    // Keep the protocol's inference inputs explicit at this shared worker boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn transcribe(
        &mut self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        audio_start_sample: u64,
        backend: BackendKind,
        timed: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<WindowTranscript, String> {
        if language.contains('\0') || context.is_some_and(|text| text.contains('\0')) {
            return Err("language/context contains a null byte".into());
        }
        // whisper-rs 0.16 set_initial_prompt leaks CString::into_raw. Tokenize
        // into an owned Vec instead; set_tokens borrows it through full.
        let prompt_tokens = context
            .filter(|text| !text.is_empty())
            .map(|text| self.context.tokenize(text, text.len().max(1)))
            .transpose()
            .map_err(|error| error.to_string())?;
        if cancelled() {
            return Err("cancelled".into());
        }
        self.pcm.clear();
        self.pcm
            .extend(samples.iter().map(|sample| *sample as f32 / 32768.0));
        let mut params = options::language_params(language)?;
        params.set_n_threads(
            std::thread::available_parallelism()
                .map_or(4, |count| count.get())
                .clamp(1, 8) as i32,
        );
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        params.set_no_context(true);
        params.set_single_segment(!timed);
        params.set_no_timestamps(!timed);
        params.set_token_timestamps(timed);
        if let Some(tokens) = &prompt_tokens {
            params.set_tokens(tokens);
        }
        let guard = AbortGuard { check: cancelled };
        // SAFETY: no mutable context access; guard outlives synchronous full.
        unsafe {
            params.set_abort_callback(Some(abort_callback));
            params
                .set_abort_callback_user_data((&guard as *const AbortGuard<'_>).cast_mut().cast());
        }
        let started = Instant::now();
        let result = self.state.full(params, &self.pcm);
        if cancelled() {
            return Err("cancelled".into());
        }
        result.map_err(|error| error.to_string())?;
        let mut segments = Vec::new();
        let mut words = Vec::new();
        let window_end = audio_start_sample.saturating_add(samples.len() as u64);
        let absolute = |timestamp: i64| {
            audio_start_sample
                .saturating_add(timestamp.max(0) as u64 * 160)
                .min(window_end)
        };
        for index in 0..self.state.full_n_segments() {
            let Some(segment) = self.state.get_segment(index) else {
                continue;
            };
            let text = segment
                .to_str()
                .map_err(|error| error.to_string())?
                .trim()
                .to_owned();
            if text.is_empty() {
                continue;
            }
            let start = if timed {
                absolute(segment.start_timestamp())
            } else {
                audio_start_sample
            };
            let end = if timed {
                absolute(segment.end_timestamp()).max(start)
            } else {
                window_end
            };
            if timed {
                let mut pieces = Vec::new();
                for token_index in 0..segment.n_tokens() {
                    let Some(token) = segment.get_token(token_index) else {
                        continue;
                    };
                    if token.token_id() >= self.context.token_eot() {
                        continue;
                    }
                    let data = token.token_data();
                    pieces.push(TimedPiece {
                        bytes: token
                            .to_bytes()
                            .map_err(|error| error.to_string())?
                            .to_vec(),
                        start_sample: if data.t0 >= 0 {
                            absolute(data.t0).max(start).min(end)
                        } else {
                            start
                        },
                        end_sample: if data.t1 >= 0 {
                            absolute(data.t1).min(end)
                        } else {
                            end
                        },
                    });
                }
                words.extend(words_from_pieces(&pieces).map_err(|error| error.to_string())?);
            }
            segments.push(TimedSegment {
                text,
                start_sample: start,
                end_sample: end,
            });
        }
        Ok(WindowTranscript {
            text: segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            segments,
            words,
            detected_language: if language.is_empty() || language == "auto" {
                whisper_rs::get_lang_str(self.state.full_lang_id_from_state()).map(str::to_owned)
            } else {
                None
            },
            transcribe_secs: started.elapsed().as_secs_f32(),
            audio_secs: samples.len() as f32 / 16000.0,
            backend,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abort_guard_propagates_cancel_and_catches_panic() {
        let check = || true;
        let guard = AbortGuard { check: &check };
        assert!(unsafe { abort_callback((&guard as *const AbortGuard<'_>).cast_mut().cast()) });
        let panics = || -> bool {
            panic!("fixture");
        };
        let guard = AbortGuard { check: &panics };
        assert!(unsafe { abort_callback((&guard as *const AbortGuard<'_>).cast_mut().cast()) });
    }
}
