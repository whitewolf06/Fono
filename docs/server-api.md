# Fono Server API v2

The standalone server exposes speech recognition without Tauri or desktop
settings. The machine-readable contract is
[`apps/fono-server/openapi.json`](../apps/fono-server/openapi.json); authenticated
clients can also read `/openapi.json` and the self-contained `/docs` page.

Keep the default loopback listener unless access is restricted by a protected
network or tunnel. See [the network contract](server-security.md). The default
Cargo build provides CPU recognition; the [CUDA image](server.md) adds supervised
GPU workers. A successful API test does not confirm a permanent service installation.

## Discovery and authentication

`GET /healthz` is the only public route. It returns 200 when the default model is
ready, or 503 while starting, failed, or stopping. `GET /v1/health` requires a
bearer token and reports model readiness and aggregate queue state.

Use `Authorization: Bearer <token>` for all other routes. Recognition tokens can
read capabilities, inventory, documentation and jobs. Admin tokens additionally
access metrics and model load/unload. `GET /v1/capabilities` reports API version,
worker protocol, body limits, formats, available devices and enabled feature
gates. `GET /v1/models` reports installed public model IDs, engine, languages,
size, placement, live runtime state and per-model feature support. Local paths
and native errors are never included.

Inventory is a bounded filesystem observation. `disk_state=present_unverified`
means that the expected size/layout is present; it does not claim that a fresh
SHA check was performed. Loads verify the selected model's pinned artifacts.
Runtime states are `ready`, `busy`, `loading`, `retiring`, `failed`, or `unloaded`.

## Synchronous recognition

```sh
curl --fail-with-body \
  -H "Authorization: Bearer $FONO_TOKEN" \
  -H 'Content-Type: audio/wav' \
  --data-binary @speech.wav \
  'http://127.0.0.1:17832/v1/recognize?language=ru&timestamps=segments'
```

`POST /v1/recognize` accepts complete WAV or Ogg/Opus bodies, or
`audio/L16;rate=16000;channels=1`: signed **big-endian** 16-bit mono PCM at 16 kHz.
WAV PCM byte order is handled by its decoder. Query parameters select an
installed `model`, `language=auto|ru|en`, `timestamps=none|segments|words`,
`decoding=fast|accurate`, and `priority=interactive|batch`. Omitted model uses the
configured default. Per-model capabilities determine which options are valid.

`X-Fono-Glossary` is a base64url UTF-8 JSON array of strings. Decoded JSON is at
most 4096 bytes; native prompt policy additionally caps it at 32 terms and 896
term bytes. Unsupported glossary, prompt or decoding options fail explicitly.
The server never logs the glossary, prompt, audio or transcript.

The response includes actual model/backend, audio and wall durations,
recognizer-generated segments and the request ID. Segment and word fields use
`start_ms`, `end_ms`, and `text`; unavailable internal queue/load timings remain
null. No subtitle times are inferred from text length. Disconnect cancels the
request; the admission lease remains held until native work actually stops.

## OpenAI-compatible transcription

```python
from openai import OpenAI

client = OpenAI(base_url="http://127.0.0.1:17832/v1", api_key=token)
with open("speech.wav", "rb") as audio:
    result = client.audio.transcriptions.create(
        model="whisper:tiny", file=audio, language="ru"
    )
print(result.text)
```

`POST /v1/audio/transcriptions` uses the batch scheduler and accepts multipart
`file`, required public `model`, optional `language`, `prompt`,
`response_format`, `timestamp_granularities[]`, and `temperature`. Complete WAV,
MP3, FLAC, Ogg/Vorbis and Ogg/Opus files are supported. Formats are `json`, `text`,
`srt`, `vtt`, and `verbose_json`; verbose JSON uses seconds and includes only
the requested segment/word arrays (segments by default). Temperature from 0
through 1 is accepted and ignored. An initial prompt is limited to **224 UTF-8
bytes** and requires a model with initial-prompt support. This conservative byte
limit keeps accepted text within the native prompt budget; larger values are
rejected without truncation. The glossary retains its separate 32-term,
896-term-byte limit and 4096-byte decoded JSON envelope. OpenAI-style errors
contain `message`, `type`, `param`, `code`, and `request_id`.

## Compatible asynchronous jobs

- `POST /v1/transcriptions`: multipart `audio`, optional `language` and `model`.
- `POST /v1/transcription-jobs`: JSON `pcm_samples`, `language`, optional `model`.
- `GET /v1/transcription-jobs/{id}`: current state and final result.
- `POST /v1/transcription-jobs/{id}/cancel`: explicit cancellation.

Both submission forms return 202. Jobs run as batch work, retain protocol 1
result fields for Windows compatibility, and have a lifecycle independent of an
HTTP upload connection. Limits cover queued work and per-token inflight work.
Interactive inference takes priority at the next batch window boundary.

## Administration and metrics

Model management requires an admin token and `models.client_model_load=true`
(default false). `POST /v1/models/{id}/load` verifies the selected installed
artifact and loads it within pool capacity. It never downloads files.
`POST /v1/models/{id}/unload` only retires idle models and cannot unload the
configured default model. Busy work is rejected with 429; supervised native
cleanup continues to hold its pool reservation and may report `retiring`.

`GET /metrics` requires an admin token and `metrics.enabled=true`. Its bounded
Prometheus output exposes global HTTP request/error/inflight/duration metrics,
pool reservations, aggregate job states and active operations. It has no labels
derived from URLs, token identities, model paths, audio or transcript contents.
Queue/native timing histograms, RTF, status counters, driver memory and JSON
events are described in [the observability contract](server-observability.md).

## Errors and limits

All responses include `X-Request-Id`; clients may supply a safe ASCII value up
to 128 characters. Recognition errors contain stable codes, safe messages and
retryability. Common statuses are 400 invalid request, 401 token missing or
revoked, 403 insufficient scope/disabled gate, 404 model missing, 408 total body
reception deadline, 413 size/duration limit, 415 unsupported audio, 429 busy with
`Retry-After`, and 503 model not ready. Body reception is bounded separately
from native loading, decoding and recognition time.

Server deployment, image publication and owner acceptance remain separate
operations. Automated public-WAV replay does not replace microphone, hotkey,
Windows insertion or production acceptance.
