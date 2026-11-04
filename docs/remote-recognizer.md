# RemoteRecognizer (RC-01)

`fono_stt::remote::RemoteRecognizer` implements the shared recognizer contract.
`SttEngine::ensure_remote_loaded` prepares or reuses the selected remote engine
without opening local weights or starting a local GPU worker. The legacy
`connect_remote` entry point delegates to it. Source changes require an idle
engine; an active transcription or retained backend handle returns busy.
Discovery completes before retiring the previous source. The old worker must
confirm exit and I/O cleanup before the remote source becomes ready.

## Desktop source selection (RC-02)

Persisted settings default to `stt_source: "local"` for existing installations.
The server source uses `stt_source: "fono_server"` and `remote_stt`:

```json
{
  "url": "http://127.0.0.1:17833",
  "model": { "engine": "whisper", "variant": "large_turbo" }
}
```

An optional `ca_pem` contains the public CA certificate. A token is never part
of this object: the desktop blocking adapter reads the existing OS credential
`Fono/stt-server-token`. Missing credentials fail preparation without selecting
a local fallback. The credential editor/IPC and server controls are RC-06/RC-05.

The existing settings transaction prepares the new engine before persisting its
source, and restores the old configured source when persistence fails. Active
dictation, pending results, scheduler work and queued API jobs fence changes.
Saved local model, acceleration, thread count and residency preferences survive
the server round trip. Returning to local with no model clears the remote route
and leaves STT unloaded until a local model is selected.

Remote source disables local model preload/recovery, capture pretranscription
and adaptive GPU monitoring. A delayed startup preload rechecks the current
settings under the transaction before loading anything. Standard dictation,
diagnostics and voice-command transcription use the configured source; capture
and processing remain on the desktop. Forwarding local API jobs and background
segments through a remote server belongs to RC-11: those API jobs currently fail
explicitly in remote mode, and never load a dormant local model.

## Transport and discovery

- Supply a server root URL and a bearer token to `RemoteEndpoint::new`.
- HTTPS uses rustls with certificate and hostname verification. For a private CA,
  pass the PEM bytes to `with_ca_pem`; the client never disables verification.
- Plain HTTP is accepted only on loopback for an SSH tunnel. `localhost` resolves
  explicitly to loopback. A LAN address such as `http://192.168.1.8` is rejected.
- URLs cannot contain credentials, query parameters, fragments or an API prefix.
  Ambient proxies, redirects, automatic retries and decompression are disabled.
- Discovery reads `/v1/capabilities` and `/v1/models` using the same connection pool.
  It requires Fono Server API >= 2, L16 support, cancellation on disconnect and an
  installed selected model. Model features and server limits form a snapshot;
  discovery does not load a server model or substitute another one.

`RemoteTimeouts` defaults to 1.5 seconds to connect and a response deadline of
`5 seconds + audio duration`. Discovery uses the response base deadline. Both
upload and reading the complete response are inside the deadline. At most two
requests per client can be active; additional work receives a busy error.

## Recording preflight (RC-03)

After physical capture starts, a separate blocking worker checks `/v1/health`
with authentication and an **800 ms network deadline**, parallel to audio and
focus preparation. It uploads no audio, does not load a model, and does not wait
on the interactive STT scheduler. Local source starts no probe.

The pipeline retains a typed, operation-scoped `remote_check` snapshot. Checking
and completion publish through `backend-event-v1` (`remote_server_check`); the
desktop snapshot also carries the state so a newly opened overlay can recover
it. Only the current, editable capture can publish. Stop cancels a pending
check using its own token, preserving completed results for the frozen session.
Cancel, abort and terminal completion retire the slot. A late response cannot
change another capture, its status, PCM, phase or controls.

Readiness failures remain advisory: the recording continues and Stop/Cancel
remain available. RC-04 decides whether to ask, attempt the server, or use the
configured CPU reserve at Stop. The indicator shows localized status while recording and
keeps the silence countdown. No URL, device name, server error text or bearer
is copied into the status event or preflight snapshot.

The server's health list contains preloads rather than its entire inventory.
A missing selected model therefore means `unknown`, not "model absent". Valid
503 responses distinguish startup/failure from transport errors. The current
server reports stopping as starting; clients cannot infer a definite shutdown
from that older response.

For an explicit metadata-only Kari check, pass `-` instead of a WAV and add
`--health-only` to the file-only example below. It reads the existing token file
without printing its contents and sends just the health request.

## Retained recording and CPU reserve (RC-04)

`remote_stt.fallback` defaults to `"none"` and supports three policies:

- `"none"`: attempt the server. If preparation or recognition fails, keep the
  recording and offer Retry, Recognize locally and Cancel.
- `"ask"`: at Stop, ask for the source before sending the recording. Retry
  explicitly selects the server; Recognize locally selects the CPU reserve.
- `{ "local": { "engine": "giga_am", "variant": "v3_e2e_rnnt_int8" } }`: use this exact
  installed model when preflight reports an unavailable server, or after one
  failed server attempt. A failed local attempt keeps the same recording and
  offers the explicit actions; it never loops automatically.

The local action uses the model named by the Local policy, or the saved local
selection for None/Ask. Missing files, unsupported language or a load failure
leave the recording available for server retry and cancellation. No model is
downloaded. The reserve loads only when used, always on CPU; saved source,
local model, GPU acceleration and residency preferences are unchanged. CPU
weights follow the existing engine lifetime; the GPU residency policy still
applies only to GPU models. The next server attempt restores the remote route
through the normal scheduler and source gate.

Only the original operation owns the retained mono 16 kHz PCM, bounded to the
five-minute recording limit (9.6 MB of samples). Audio is not persisted and is
lost when Fono exits. The snapshot/event contains duration and typed state,
never PCM, URLs, tokens or server response text. The pending operation holds
updater admission and blocks a new recording or source change until resolved.
Cancellation clears it; physical inference retains its own lease until the
worker actually finishes. Concurrent retry actions cannot run two attempts.

Retries preserve the frozen processing choices and captured destination.
Successful recognition rejoins ordinary processing, copy and checked insertion;
history is created once after the accepted action. CPU reserve results carry
`metadata.fallback: true` plus the actual local model/backend. Generation time
continues to mean Stop to final text, including time spent choosing or retrying.

Manual acceptance: configure each policy, then interrupt the server before
recording, during capture and during upload/recognition. Check the retained
recording can be retried, recognized locally or cancelled; repeat a failed
retry and double-click Retry. Move focus to another field before resolving:
insertion must be blocked and the text must remain available for Copy. Check
one history entry after success, no entry after Cancel, and no late result in
the next session. Repeat with the overlay disabled using the main window.
Automated fixtures do not establish microphone, hotkey or external-field acceptance.

## Recognition and ownership

`RecognitionOptions` specifies language, decoding strategy and glossary. PCM is
16 kHz mono signed i16. The client explicitly writes **big-endian** samples for
`audio/L16;rate=16000;channels=1`, independent of the host byte order.
Requests use the selected public model ID, interactive priority and the native
`/v1/recognize` route. Glossary is bounded base64url JSON in a sensitive header.
The native endpoint has no context prompt or per-request thread-count option;
these options are rejected rather than silently discarded.

`WindowOptions` requests segment timestamps and converts milliseconds to absolute
sample positions using `audio_start_sample`. Backend metadata comes from the
response; `Transcript.device` identifies the remote device (`remote:cuda:0`).
Remote work never reports local GPU residency. `SttEngine::health()` reports
client readiness/busy state; it is not an online server health probe (RC-03).

These are blocking host entry points: call them from a worker or `spawn_blocking`,
never a Tauri UI/async executor thread. A shared Tokio runtime owns asynchronous
HTTP work. Cancellation drops the active HTTP future, so the server receives a
disconnect. Admission and the concurrency permit remain owned by that task until
future destruction finishes; the blocking caller then receives cancellation.
Cancelled results cannot publish into the next operation. The server separately
retains its native inference lease until actual worker completion.

Remote errors carry a bounded classification, HTTP status and optional numeric
Retry-After. Conversion to `CoreError` preserves authorization, busy, model,
network and worker failure codes. Error formatting never copies the URL, bearer,
glossary, audio, transcript or arbitrary server error body. Desktop presentation
currently uses its existing generic STT error; detailed IPC/UI mapping is RC-06.

## Explicit file-only smoke check

Use a public PCM16 mono 16 kHz WAV and an existing token file. This command neither
records the microphone nor starts the desktop UI. Output contains only timing,
device and nonempty-result metadata, without the token or recognized text.

```powershell
# Start the existing protected Kari SSH tunnel, then:
cargo run --locked -p fono-stt --no-default-features --example remote-recognize -- `
  http://127.0.0.1:17833 <private-token-file> <public-fixture.wav> whisper:large_turbo
```

Contract tests in `fono-stt` use real HTTP listeners for success, endian order,
metadata gates, auth, overload, deadlines, disconnect, cancellation, response
limits, redirects and admission ownership. Server tests additionally use the
production router with fake inference and a real listener, and the real TLS host
with generated certificates and correct/incorrect private CAs. These checks do
not replace manual acceptance of hotkeys, microphone, overlay or source switching.
