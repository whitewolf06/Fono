# Windows client and Fono Server acceptance

This is the owner checklist for RC-08 and the remote-recognition flow. Record
results for the exact desktop commit and server build being tested. An unchecked
item is pending acceptance, even when its lower-level automated tests pass.

Use a native desktop dev or built Tauri application with Rust. Browser preview
uses a mock runtime and cannot establish server routing, microphone, global
hotkey, focus, external insertion or native-window behavior. Installer packaging
and acceptance on a clean Windows installation are separate checks.

The owner records the microphone and operates Fono and other applications.
Agents may use synthetic fixtures and public WAV files without operating the
owner's application. No microphone or personal recording is needed for automated
transport and queue tests.

## Automated client-server gates (TST-02-WIN)

`cargo test --locked -p fono-server --lib runtime::client_e2e` exercises the
Windows client's `RemoteRecognizer` and `RemoteTranscriptionClient` against an
ephemeral loopback listener with the production router, authentication, discovery,
audio decoding, admission and job queue. Only the inference ports are controlled
fakes. On Windows, invoke Cargo through `scripts/test-native.ps1` so the verified
native runtime is staged beside test executables:

```powershell
./scripts/test-native.ps1 -CargoArguments @('--locked', '-p', 'fono-server', '--lib') -TestArguments @('runtime::client_e2e')
```

These tests cover PCM/options and actual backend metadata, timestamp offsets,
authentication and token revocation, typed errors, advertised limits, explicit
retry, multipart upload and job polling. Cancellation, deadline and TCP-disconnect
cases deliberately keep inference alive after the client stops waiting. They
verify server activity and the per-client admission remain held until actual
completion, a premature retry receives 429, and a later explicit retry cannot
receive the cancelled result.

A separate real-inference gate runs the same native clients through production
`ServerJobs` and `ModelPool` on CPU, including interactive and queued file
transcription. It requires the SHA-256-pinned Tiny model and public JFK WAV from
`apps/fono-server/tests/fixtures/remote-client-cpu.json`. Preparing resources does
not access the network unless `--download` is explicit. Existing verified local
resources can be reused without copying:

```powershell
node scripts/remote-client-cpu-resources.mjs --model build/stt-quality/ggml-tiny.bin --wav build/server-panel-linux-qa/jfk.wav
$env:FONO_TST02_MODEL = (Resolve-Path build/stt-quality/ggml-tiny.bin).Path
$env:FONO_TST02_WAV = (Resolve-Path build/server-panel-linux-qa/jfk.wav).Path
$env:FONO_TST02_REPORT = Join-Path (Get-Location).Path 'build/remote-client-cpu/windows.json'
./scripts/test-native.ps1 -CargoArguments @('--locked', '-p', 'fono-server', '--lib') -TestArguments @('runtime::client_cpu::real_cpu_remote_client_roundtrip', '--ignored', '--exact', '--nocapture')
```

On Linux, set the same absolute resource paths and run:

```sh
cargo test --locked -p fono-server --lib runtime::client_cpu::real_cpu_remote_client_roundtrip -- --ignored --exact --nocapture
```

The resource-gated test is ignored by ordinary Cargo runs, but both Windows and
Linux CI explicitly execute it after fixture verification. Missing or changed
resources fail the explicit gate. Its optional JSON receipt contains hashes and
numeric measurements, without tokens, endpoints or transcripts. These automated
gates do not establish microphone, hotkey, insertion, overlay, GPU, clean Windows
installation or owner acceptance; the checklist below remains separate.

## Preparation

- Record desktop version/commit, server version/API version, selected model and
  whether the test uses a protected connection or the existing Kari SSH tunnel.
  Do not attach the endpoint, token, certificate path, personal audio or transcript.
- Prepare one short public speech WAV, a longer speech file, and a malformed
  file. Keep the originals; transcription must not modify them.
- Use a recognition token and a server model already installed. Check the
  connection in Settings, save the token and source, and keep the local model
  and GPU preferences available for the return-to-local check.
- For Kari, the protected loopback tunnel may use port 17833. The Windows local
  API and the server tunnel must not compete for port 17832. Follow the existing
  [connection contract](../remote-recognizer.md#connection-settings-rc-05) and
  [server network rules](../server-security.md).
- Configure an exact installed local model if testing the CPU reserve. No test
  should download a model or silently change the saved acceleration preference.

## Scenario matrix

| Scenario                                    | Recognition route                | Other behavior to preserve                                                  |
| ------------------------------------------- | -------------------------------- | --------------------------------------------------------------------------- |
| Ordinary dictation                          | Selected Fono Server model       | Capture, VAD, processing, history and insertion stay on Windows.            |
| Processing test with trial dictation        | Same configured recognizer       | Draft test stays isolated from history, last result and external insertion. |
| Voice commands                              | Configured recognizer            | Existing command proposal and confirmation remain in effect.                |
| Edit selected text with a voice instruction | Trial-dictation recognition path | Selection ownership, cancellation and explicit acceptance remain in effect. |
| Audio files                                 | Server asynchronous batch job    | Desktop queue, progress, cancellation and export remain available.          |
| Lira wake detector                          | Local wake worker                | Choosing the server does not move the detector to the server.               |
| Microphone diagnostic test                  | Local capture                    | Testing levels does not send the sample for server recognition.             |

For every remote scenario, check the selected model and actual backend using
the available server job/status data or safe history metadata. A saved local
model must not be loaded as an implicit replacement. A normal lack of per-stage
timing is unknown, not zero and not a network-time estimate.

## Interactive scenarios

1. With Fono Server selected, dictate a short phrase into three different
   applications. Check both hold and toggle hotkey modes, the main window and
   overlay, one result per accepted action, and the existing focus safeguards.
   Repeat after switching back to Local and then to Fono Server.
2. Run a processing test with trial dictation. Check the draft instruction is
   used, no history or last-session result is created, and no text is inserted
   into another application. Close the form during recognition, then start a new
   trial. The old completion must not replace the new trial's result.
3. Dictate a voice command. Check the proposal is shown through the existing
   confirmation flow. Cancel during recording and recognition; no cancelled
   operation may publish a late proposal or interfere with the next command.
4. Select text, start editing with a voice instruction, and check the existing
   preview and acceptance flow. Repeat with Cancel and with the original field
   or selection changed. A late instruction must not edit a newer selection or
   produce a second external insertion.
5. With the server unavailable, repeat the ordinary-dictation recovery cases
   below. Test the other interactive scenarios for a usable error and a clean
   next operation; do not infer that the ordinary-dictation reserve UI applies
   to every workflow.

## Compatibility and server changes (RC-10)

Use a disposable test server or the automated HTTP fixtures for incompatible
responses. Do not downgrade or reconfigure the shared Kari service solely to
complete these checks. Record which cases are automated and which are observed
in the native desktop application.

1. With a saved Fono Server source, restart Fono. Confirm startup performs only
   metadata preparation, reports a useful failure for an unavailable or
   incompatible server, and leaves the saved source and local preferences intact.
   No microphone capture, local model load or reserve activation should occur.
2. Inspect a minimal API-version-1 server response. The result should explain
   that Fono Server needs updating. No recognition or batch upload may occur,
   even when a previous connection was successful.
3. Remove a required interactive capability: L16, cancellation on disconnect or
   fast decoding. Check the unsupported-capabilities result is distinct from an
   old API version and that the client sends no audio. Separately remove WAV or
   jobs support and verify file uploads fail before submission.
4. Remove glossary, timestamps or accurate decoding globally and then only from
   the selected model. An unsupported requested option must fail before upload;
   a supported basic request should still work. Default requests must not send
   unadvertised optional parameters such as `timestamps=none`.
5. Keep a prepared client, then restart the test server with an older API or
   fewer capabilities. Try the next dictation and file upload without saving
   settings again. Fresh metadata must prevent an upload that the new server
   cannot support. Restore the server and check the next valid operation works.
6. Test an API version above 2 with the required response fields/capabilities and
   additional unknown fields or optional feature values. It should remain usable.
   Product release versions may differ between desktop and server; API, worker
   protocol and asynchronous result protocol must not be compared as one version.
7. Cancel metadata preparation or attempt to change the selected source while
   it is pending. If admission rejects a change as busy, confirmed settings must
   remain selected without a partial commit. A late response must not overwrite
   a newer source, validate a different connection draft or publish a result into
   a later operation. Existing credential, updater-admission and
   retained-recording checks still apply.

## Files, priority and cancellation

1. Disable the Windows local API listener, then add public WAV/MP3/FLAC/OGG files
   through Files. The selected Fono Server model must process them while the
   optional local API stays disabled. Observe desktop and server job progress.
   Original file paths and contents must remain unchanged.
2. Queue several files. Cancel a queued file, a preparing file and an active
   server job. Repeat with Cancel all. Once its identifier is known, the client
   attempts bounded explicit server cancellation; cancelling only a polling
   request does not cancel an asynchronous job. A late result must not become a
   completed desktop file result. An unreachable server can prevent cancellation
   from being delivered; inspect its queue after restoring the connection.
3. While a longer file is running, start ordinary dictation. Interactive work
   must get priority at the next available inference boundary. The batch job
   must continue or resume without duplicate text, lost checkpoints or an
   automatic second submission. Repeat while another file is queued.
4. Interrupt the connection after a server job has been accepted. Check the
   desktop stops waiting with a usable error or cancellation state and remains
   usable for the next file. Do not assume a lost upload response proves that
   the server rejected the upload. If the response containing its identifier was
   lost, the client cannot address that job for cancellation. Inspect the server
   queue after restoring the connection; bounded best-effort cancellation cannot
   guarantee that such a job has stopped. Do not automatically retry a
   non-idempotent submission and create duplicate jobs.
5. Check malformed, unsupported and oversized files. Failure must stay attached
   to the relevant file, without exposing the endpoint, credentials or arbitrary
   server error text. Retry a valid file after each failure.
6. Export TXT after completion. Export SRT/VTT only when the server returned real
   timestamp segments; otherwise those formats remain unavailable. Listen across
   segment boundaries, pauses and the final audio tail. Do not fabricate subtitle
   times from text length. Also run the applicable
   [file acceptance checks](file-transcription.md).
7. A queued or active file fences source/model/token changes and update
   installation for the actual worker lifetime. After cancelling, check that a
   new operation works and no late result is published. Exiting while a job is
   active must follow the existing shutdown/cancellation rules.

The loopback desktop API is a separate RC-09 scope. File forwarding alone does
not establish that external requests to Windows port 17832 forward to the server.
Server batch jobs use the [asynchronous API](../server-api.md#compatible-asynchronous-jobs).

## Recovery, metadata and local boundaries

1. Test each ordinary-dictation reserve policy: None, Ask and the configured CPU
   reserve. Interrupt the server before recording, during capture and during
   recognition. Keep Retry, Recognize locally and Cancel usable as described in
   the [retained-recording contract](../remote-recognizer.md#retained-recording-and-cpu-reserve-rc-04).
2. Retry twice, double-click Retry and cancel before a delayed response arrives.
   Check exactly one accepted history entry, no entry from a cancelled operation,
   and no response leaking into the next recording. Change the destination field
   before retrying: automatic insertion must be blocked and Copy must remain usable.
3. Inspect a remote history item and a CPU-reserve item. Check actual model,
   backend, fallback flag and measured durations. Rename/reconfigure the server
   and confirm older items retain their own facts. Read an older history entry
   without the optional remote fields.
4. Export the local diagnostic report. It must distinguish the selected source
   from the actual last result and exclude endpoint, token, certificate paths,
   hardware/device names, audio and transcript content.
5. If the separately enabled Lira beta is under test, observe that its local
   detector still works according to its own acceptance contract. Do not enable
   the legacy arbitrary wake-phrase gate as part of RC-08.
6. Run the microphone level test while the server is unavailable. It should still
   measure local capture without loading a speech model or sending recognition
   audio to the server. This does not establish recognition accuracy.

## Recording the result

For each matrix row, record pass/fail/pending, the action, observed result,
desktop/server versions and any safe request/job identifier. Keep automated
HTTP/queue evidence separate from owner microphone/hotkey/window acceptance.
Do not mark RC-08 or the complete Windows/server integration accepted until the
owner has confirmed the required native scenarios. Publication, deployment,
installation and clean-Windows acceptance are separate decisions.
