# RemoteRecognizer (RC-01)

`fono_stt::remote::RemoteRecognizer` implements the shared recognizer contract.
`SttEngine::connect_remote` prepares an unloaded engine without opening local
weights or starting a GPU worker. Source selection, replacing an active local
backend, settings, credentials IPC and desktop controls are subsequent RC tasks.

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
