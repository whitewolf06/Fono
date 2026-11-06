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
a local fallback. The connection form and credential IPC are described below.

The existing settings transaction prepares the new engine before persisting its
source, and restores the old configured source when persistence fails. Active
dictation, pending results, scheduler work and queued API jobs fence changes.
Saved local model, acceleration, thread count and residency preferences survive
the server round trip. Returning to local with no model clears the remote route
and leaves STT unloaded until a local model is selected.

Remote source disables local model preload/recovery, capture pretranscription
and adaptive GPU monitoring. A delayed startup preload rechecks the current
settings under the transaction before loading anything. Standard dictation,
trial dictation, voice-command transcription and voice instructions for editing
selected text use the configured source; capture and processing remain on the
desktop. File forwarding is RC-08. Forwarding local API jobs is the separate
RC-09 scope; those API jobs currently fail explicitly in remote mode and never
load a dormant local model. Sending background capture segments is RC-11.

## Connection settings (RC-05)

Open Settings → Audio and choose **Fono Server**. The local model and GPU
preferences remain saved for switching back. Enter the server root URL, a display
name and a token. For Kari, use the existing SSH tunnel at
`http://127.0.0.1:17833`; HTTPS is required for non-loopback endpoints. An optional
public CA PEM supports private certificates without disabling TLS verification.
OpenAI-compatible servers and Opus compression remain separate tasks; this
connection sends the existing PCM format.

### Managed SSH connection

Choose **SSH** as the connection method to let Fono establish the protected
channel itself. Enter the SSH host, port and user, select an existing private-key
file (or use an already unlocked SSH agent), and keep the server API port at
17832 unless the server administrator configured another port. The API is
forwarded only from the remote loopback interface. No terminal command or
separately started tunnel is needed after saving the connection.

Request the server's public host key in the form, compare its SHA-256 fingerprint
with a trusted value supplied by the server administrator, and explicitly trust
that key. Fetching a key is an observation, not authentication; Fono does not
silently trust a first key or replace a changed one. Check the connection with
the API token, choose the installed model, save the token and save the settings.
Changing the SSH target, user, key or port invalidates the check and cannot reuse
the stored API credential for a draft destination.

The Windows OpenSSH client is required. Fono starts hidden, owned SSH processes
with strict host-key checking and public-key authentication. It does not store
private-key contents or an SSH password, run a remote shell, alter the user's SSH
configuration, or adopt an independently started tunnel. A protected private key
must already be available through the SSH agent. The API bearer remains in
Windows Credential Manager. SSH addresses, usernames and key-file paths are
native settings and stay out of browser persistence and diagnostic exports.

Startup preparation, recording preflight and file jobs automatically use the
saved channel. A broken connection remains a failed attempt: audio POSTs are not
replayed automatically. The next explicit operation opens a new SSH connection.
Switching to a direct connection or the local recognizer retires Fono's relay;
application shutdown closes its owned sockets and processes. Existing manual
tunnels remain separate.

For a first connection: enter the token, **Check connection**, select an installed
server model and a reserve policy, **Save token**, then save the settings section.
Checking a draft does not save it, send audio, load a model or switch the engine.
The reply shows the actual server version, devices, inventory and readiness.
When a model is selected, the check validates that model's required capabilities.
If it was removed or is incompatible, a separate metadata request refreshes the
replacement inventory while preserving the selected-model error. A late inventory
reply cannot validate a changed draft.
Server models are read-only; downloads and deletion remain server administration.
Local GPU controls are hidden while using the server. An installed local model
may be selected for the CPU reserve from RC-04.

The password field is transient and never displays the stored token. Save and
Delete use Windows Credential Manager. Saving settings explicitly prepares the
chosen endpoint using that credential. Metadata checks with an omitted token
reuse it only when the normalized URL and CA match the saved connection;
checking a changed destination requires entering a token. Typing an address
does not start a request. Late results cannot validate a newer draft.

The browser runtime uses marked demo responses and makes no server requests.
It keeps connection drafts and credential presence only in memory; URLs, CA
contents and tokens are excluded from browser persistence. Remote-only setup
also works in onboarding; diagnostics and the recognition card use the selected
source instead of demanding dormant local weights.

The native boundary exposes async `get_remote_stt_status`,
`test_remote_stt_server`, `list_remote_stt_models`, `set_remote_stt_token` and
`clear_remote_stt_token`. Network and credential work runs on admitted blocking
workers. Credential changes share the configuration/settings fences with capture,
inference and pending results. Status returns only credential presence; there is
no token getter. Public source preferences use the existing settings transaction.

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

## Version and capability compatibility (RC-10)

Compatibility is checked when inspecting a connection, preparing a saved remote
source at application startup, recording preflight, and before each interactive
audio or file-job upload. Startup preparation exchanges metadata only; it does
not capture audio, load local weights or activate the saved local reserve.
Preparing a client once does not authorize future uploads against stale server
capabilities. The client refreshes capabilities and the selected model before
each upload, including after a server restart or downgrade.

The version fields describe different contracts:

| Field                         | Purpose                                              | Client rule                                                                                                 |
| ----------------------------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Product `version`             | Human-facing Fono release                            | May differ from the desktop version; equality is not required.                                              |
| `api_version`                 | Native HTTP API generation                           | Versions below 2 fail as incompatible; versions 2 and above must still pass the required capability checks. |
| `worker_protocol`             | Server-to-native-worker protocol, currently 4        | The server owns this handshake; the HTTP client does not require its local worker protocol to match.        |
| Job-result `protocol_version` | Compatible asynchronous result envelope, currently 1 | Validated independently when reading a completed file result.                                               |

API versions above 2 are accepted only with a compatible response shape and the
required route capabilities. Unknown additive fields and optional feature values
do not by themselves make a server incompatible. A minimal old API response can
still be recognized as requiring a server update, rather than being reported as
an unrelated malformed model catalog.

Interactive recognition requires the native L16 format, cancellation on
disconnect and the baseline fast decoding mode. Batch files instead require WAV
upload and asynchronous jobs; they do not depend on the interactive cancellation
contract. Installed-model and bounded-upload requirements still apply to both.
Missing required capabilities fail as `UnsupportedCapabilities` before audio is
sent. An old API version uses the separate incompatible-server error and directs
the user to update Fono Server.

Optional glossary, timestamp and decoding support is the intersection of the
server-wide and selected-model declarations. Unsupported requested options fail
before upload; unknown options are not guessed or silently enabled. The client
omits the default `timestamps=none` parameter rather than sending an unadvertised
optional value. A metadata or compatibility failure preserves the chosen source
and local preferences for explicit recovery.

## Recording preflight (RC-03)

After physical capture starts, a separate blocking worker refreshes capabilities
and the selected model, then checks `/v1/health` with authentication. The complete
probe shares an **800 ms network deadline**, parallel to audio and focus
preparation. It uploads no audio, does not load a model, and does not wait on the
interactive STT scheduler. Local source starts no probe.

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

## Session facts, history and diagnostics (RC-07)

Every successful remote `Transcript` carries optional typed `remote` facts from
that request: the actual model, physical backend and protocol device code.
These facts travel with the transcript through processing and acceptance; they
are not read from a shared last-response cache or today's connection settings.
The desktop archives the server display name from the frozen session, without
using its URL as a fallback label. Local reserve success records its actual local
model/backend and `fallback: true`, with no stale server fields.

History uses `backend: "remote"` plus optional `remote_name`, `remote_device`,
`remote_backend`, `request_duration_ms`, `server_total_duration_ms`,
`network_duration_ms` and `decode_duration_ms`. The request duration is measured
on the client around the complete request, including client scheduling and
serialization. Server total can include queueing and loading. Neither is a
network-only or inference-only measurement. Only explicit server `decode_ms`
populates remote `transcribe_secs` and `recognition_duration_ms`; missing decode
and network measurements remain unknown. No RTT-minus-total estimate is used.

The fields are optional additions to the existing SQLite JSON payload. Old
records remain readable without a database migration. Known legacy `remote:*`
device codes can identify a remote route; old server totals are not relabeled
as decode time. Explicit future backend values stay unknown, and malformed or
contradictory optional facts are discarded while keeping the history result.
Actual measured zero is retained. History hides dormant local GPU preferences
for remote results and displays request/server/individual stages separately.

The local diagnostic export distinguishes the configured source from the actual
backend of the last completed flow. A server selection never probes dormant
local weights or claims fresh server availability. Public model labels and
backend names use closed allowlists. Last-result telemetry accepts only enums,
booleans and numeric measurements: no server labels, device/hardware names,
endpoints, credentials, audio or transcript text enter the export.

Manual acceptance: make one server dictation and one local reserve dictation,
then inspect their history details and exported report. Rename/reconfigure the
server between sessions and confirm old entries retain their own facts. Check
older records remain readable. Automated storage, HTTP and rendering tests do
not establish microphone, hotkey, external insertion or real-window acceptance.

## Scenario routing and acceptance (RC-08)

Ordinary dictation prepares the frozen session's configured recognizer through
the shared scheduler. Processing-test dictation reuses that recognition path
without archiving, external insertion or last-session publication. Editing a
selection by voice reuses the isolated trial capture. Voice commands prepare the
configured recognizer through their existing interactive scheduler path, then
keep the existing proposal/confirmation workflow.

Files require server asynchronous batch jobs, with progress, explicit job
cancellation and real returned subtitle segments. This is a different lifecycle
from the interactive `/v1/recognize` request: closing a polling HTTP connection
does not cancel a submitted job. When a job ID is known, cancellation is an
explicit bounded best-effort request. If submission is accepted but its response
is lost before the ID arrives, the client cannot identify that job for cleanup;
inspect the server queue after reconnecting. Automatic resubmission could create
duplicates and is not a recovery strategy. File support must not implicitly enable
loopback API forwarding, reactivate a saved local model or turn ordinary-dictation
CPU reserve into an automatic file fallback. The optional Windows API listener
is not required for the Files screen.

Lira's wake detector stays in its local worker; the microphone diagnostic test
only measures local capture. Neither uses remote recognition because Fono Server
is selected. Enabling the separately approved Lira beta does not open the legacy
arbitrary wake-phrase gate.

Use the [Windows/server checklist](qa/remote-stt.md) for all seven scenario rows,
batch priority, cancellation, source return, reserve policies and metadata.
Automated transport and queue fixtures confirm their own boundaries. Native
microphone, hotkey, selection, overlay and external-insertion acceptance remains
an explicit owner check.

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
uses its existing generic STT error for dictation failures. Connection inspection
has separate typed results; retained recording actions handle failed dictation.

## Explicit file-only smoke check

Use a public PCM16 mono 16 kHz WAV and an existing token file. This command neither
records the microphone nor starts the desktop UI. Output contains only timing,
device and nonempty-result metadata, without the token or recognized text.

```powershell
# Start the existing protected Kari SSH tunnel, then:
cargo run --locked -p fono-stt --no-default-features --example remote-recognize -- `
  http://127.0.0.1:17833 <private-token-file> <public-fixture.wav> whisper:large_turbo
```

Append `--batch` to exercise the RC-08 asynchronous file route: WAV multipart
submission, server job polling and a validated result with any real subtitle
segments. The same example then reports `batch: true` and `local_stt_used: false`.
It does not submit a second request to the interactive recognition endpoint.

Contract tests in `fono-stt` use real HTTP listeners for success, endian order,
metadata gates, auth, overload, deadlines, disconnect, cancellation, response
limits, redirects and admission ownership. Server tests additionally use the
production router with fake inference and a real listener, and the real TLS host
with generated certificates and correct/incorrect private CAs. These checks do
not replace manual acceptance of hotkeys, microphone, overlay or source switching.
