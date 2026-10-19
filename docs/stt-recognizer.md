# Shared recognizer contract

F-01.2 routes embedded Whisper and the supervised worker mailbox through the
same infrastructure `Recognizer` trait in `src-tauri/src/stt/recognizer.rs`.
It defines a replacement factory (`load`), transcription, window transcription,
the loaded model identity, actual device and runtime capabilities. The trait is
internal to STT infrastructure; application callers keep using `SttEngine`.

`SttEngine::ensure_model_loaded` accepts the registry's `SttModelSelection` and
an operation cancellation token. Existing path-based loading remains a Whisper
compatibility entry point. Unsupported engines fail before entering a Whisper
loader. Dictation and desktop file jobs pass their snapshotted selection and
cancellation token to the typed entry point. The old settings path, IPC shape
and transcript metadata remain compatible.

Loaded handles retain their model identity and actual device. Metadata queries
do not take the embedded inference lock or consult changing settings. A worker
advertises optional decoding support from its validated hello; a legacy worker
does not silently accept nondefault decoding options. Unsupported prompt,
language-detection or window options also fail before native work.

## Ownership and cancellation

The common boundary checks cancellation before and after actual native work.
A backend that cannot interrupt decoding must still finish its current bounded
segment; a late result is then discarded. `CancellationSupport` distinguishes
cooperative implementations from cancellation between segments. The latter is
covered by a test double, not an enabled offline engine.

The common call retains update admission until native work returns. Workers
also retain their existing mailbox and cleanup leases. Existing callers own
the shared scheduler permit and operation lifecycle, including when an async
waiter is cancelled. Window operation IDs, sample offsets and context pass
through unchanged. Worker response request/operation/backend validation and
generation checks remain in their existing owners.

Loading prepares a separate handle before publishing it. A cancelled load
does not publish that replacement. Routing order, explicit GPU selection,
readiness, health, bounded recovery and GPU residency retain their previous
policies. Health and model/capability reads do not wait for inference.
Native context construction is not interrupted inside its constructor:
cancellation is checked at its boundaries, and admission and the caller's
scheduler permit remain held until construction and cleanup actually finish.

## Verification and remaining work

Contract tests cover pre-cancellation, cancelled late results for both call
types, unsupported options, offline-style CPU metadata, operation IDs and
offsets, scheduler retention during uninterruptible work, and rejection of an
unavailable engine. A supervised process fixture exercises the common worker
load/transcription path, model/device metadata and legacy hello capabilities.
It validates the protocol; it performs no neural GPU inference.

The explicitly invoked `embedded_whisper_public_wav_contract` test uses the
pinned multilingual Tiny model supplied by `FONO_RECOGNIZER_TEST_MODEL` and
the existing `en-01`/`ru-01` public FLEURS fixtures. It verifies their hashes
and compares direct Whisper with the routed recognizer for ordinary and
window text, language, timestamps, audio duration and actual device. It is
excluded from ordinary tests so they never implicitly download a model.

GigaAM/Parakeet artifacts and actual offline decoding remain F-01.3 (FONO-115).
Long-file VAD segmentation remains F-01.4. Full GPU/driver, microphone, hotkey,
overlay and installed Windows acceptance require their separate checks. No
worker wire format or binary protocol changes are introduced by this stage.
