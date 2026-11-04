//! Source selection runs at most one remote and one reserve attempt.
use std::future::Future;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Server,
    Local,
}

pub(super) async fn run<T, E, F, Fut>(
    initial: Source,
    reserve: bool,
    mut recognize: F,
) -> Result<Option<(T, Source)>, Source>
where
    F: FnMut(Source) -> Fut,
    Fut: Future<Output = Result<Option<T>, E>>,
{
    let mut source = initial;
    loop {
        match recognize(source).await {
            Ok(result) => return Ok(result.map(|result| (result, source))),
            Err(_) if source == Source::Server && reserve => source = Source::Local,
            Err(_) => return Err(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        initial_attempt, state::Store, InitialAttempt, RecoveryPhase, RecoveryReason,
        RecoverySnapshot,
    };
    use super::*;
    use crate::{
        application::dictation::DictationOutcome,
        types::{ModelId, RemoteFallback, WhisperModelSize},
    };
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn outage_before_start_during_capture_and_upload_retains_pcm_for_every_policy() {
        for stage in [0, 1, 2] {
            for policy in [
                RemoteFallback::None,
                RemoteFallback::Ask,
                RemoteFallback::Local(ModelId::whisper(WhisperModelSize::Tiny)),
            ] {
                let pcm = Arc::new(vec![1_i16, 2, 3]);
                let mut store = Store::default();
                store
                    .install(
                        RecoverySnapshot {
                            session_id: 11,
                            phase: RecoveryPhase::Recognizing,
                            reason: RecoveryReason::ServerFailed,
                            audio_secs: 0.1,
                            local_available: true,
                        },
                        pcm.clone(),
                    )
                    .unwrap();
                let calls = Arc::new(Mutex::new(Vec::new()));
                let plan = initial_attempt(&policy, stage == 0);
                let result = if plan == InitialAttempt::Ask {
                    // Ask cannot upload even when health still said ready.
                    Err(Source::Server)
                } else {
                    let initial = if plan == InitialAttempt::Local {
                        Source::Local
                    } else {
                        Source::Server
                    };
                    run(
                        initial,
                        matches!(policy, RemoteFallback::Local(_)),
                        |source| {
                            let pcm = pcm.clone();
                            let calls = calls.clone();
                            async move {
                                assert_eq!(pcm.as_slice(), &[1_i16, 2, 3]);
                                calls.lock().unwrap().push(source);
                                // Different failure points happen after an actual await,
                                // including the server disappearing after health succeeded.
                                if stage == 2 {
                                    tokio::task::yield_now().await;
                                }
                                if source == Source::Server {
                                    Err(())
                                } else {
                                    Ok(Some("local transcript"))
                                }
                            }
                        },
                    )
                    .await
                };
                match policy {
                    RemoteFallback::Local(_) => {
                        assert!(matches!(
                            result,
                            Ok(Some(("local transcript", Source::Local)))
                        ));
                        assert_eq!(
                            *calls.lock().unwrap(),
                            if stage == 0 {
                                vec![Source::Local]
                            } else {
                                vec![Source::Server, Source::Local]
                            }
                        );
                    }
                    RemoteFallback::None | RemoteFallback::Ask => {
                        assert!(result.is_err());
                        assert!(store.retry(11, RecoveryReason::ServerFailed));
                        store.release(11);
                        let retry_pcm = store.claim(11).unwrap().unwrap();
                        assert!(Arc::ptr_eq(&retry_pcm, &pcm));
                        assert_eq!(
                            calls.lock().unwrap().len(),
                            usize::from(matches!(policy, RemoteFallback::None))
                        );
                    }
                }
            }
        }
    }
    #[tokio::test]
    async fn failed_local_reserve_does_not_loop_or_discard_recording() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let result = run::<(), _, _, _>(Source::Server, true, |source| {
            calls.lock().unwrap().push(source);
            async { Err(()) }
        })
        .await;
        assert_eq!(result, Err(Source::Local));
        assert_eq!(*calls.lock().unwrap(), vec![Source::Server, Source::Local]);
    }
    #[tokio::test]
    async fn cancellation_never_starts_a_reserve_and_explicit_server_retry_does_not_fallback() {
        let mut calls = Vec::new();
        assert_eq!(
            run::<(), (), _, _>(Source::Server, true, |source| {
                calls.push(source);
                async { Ok(None) }
            })
            .await,
            Ok(None)
        );
        assert_eq!(calls, vec![Source::Server]);
        calls.clear();
        assert_eq!(
            run::<(), (), _, _>(Source::Server, false, |source| {
                calls.push(source);
                async { Err(()) }
            })
            .await,
            Err(Source::Server)
        );
        assert_eq!(calls, vec![Source::Server]);
    }
    #[tokio::test]
    async fn one_success_handoff_retires_audio_before_duplicate_actions() {
        let mut store = Store::default();
        store
            .install(
                RecoverySnapshot {
                    session_id: 42,
                    phase: RecoveryPhase::AwaitingAction,
                    reason: RecoveryReason::ServerFailed,
                    audio_secs: 0.1,
                    local_available: false,
                },
                Arc::new(vec![7_i16]),
            )
            .unwrap();
        let _pcm = store.claim(42).unwrap().unwrap();
        let output = run::<_, (), _, _>(Source::Server, false, |_| async { Ok(Some("result")) })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(output.0, "result");
        assert!(store.detach(42, None).is_some());
        store.complete(42, DictationOutcome::NoSpeech);
        assert!(matches!(
            store.claim(42).unwrap(),
            Err(DictationOutcome::NoSpeech)
        ));
        assert!(store.detach(42, None).is_none());
    }
}
