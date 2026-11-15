# Fono

[Русский](README.ru.md) · [Windows downloads](https://github.com/whitewolf06/Fono/releases/latest) · [Documentation](docs/README.md)

Voice input for Windows, with local recognition or your own Fono Server.
Record with a hotkey, insert the result into a text field, and optionally edit
or translate it through your chosen LLM connection. The interface supports
Russian and English.

This repository contains **Fono Desktop** and the headless **Fono Server**.
The latest published release is
[0.9.1](https://github.com/whitewolf06/Fono/releases/tag/v0.9.1), including Windows
installers and public Linux API images. A newer source version does not imply
another published release.

## Choose where recognition runs

|                  | Windows Desktop                                        | Fono Server                                               |
| ---------------- | ------------------------------------------------------ | --------------------------------------------------------- |
| Runtime          | Windows x64, microphone, hotkey and overlay            | Headless Linux x86_64 container; no Tauri or microphone   |
| Default GPU path | Vulkan + embedded CPU                                  | Vulkan + CPU image                                        |
| Optional CUDA    | Separately signed package, installed in audio settings | Separate CUDA image                                       |
| Client access    | Local speech or configured Fono Server                 | Bearer-token API, embedded web panel and application keys |

Auto uses Vulkan on a suitable physical discrete GPU, or CPU when none is
available. Installing CUDA does not change Auto; CUDA is selected explicitly.
GigaAM and Parakeet run on CPU. Model weights are downloaded separately.

In server mode, speech-model weights stay on the server. The Windows client
does not load local recognition weights unless an explicitly selected CPU
reserve is used. LLM processing remains a separate connection.

Standalone Windows GPU Server, Linux Desktop and macOS Desktop are outside
this release's supported distribution. The Linux container can be hosted in a
suitable Linux environment; Windows/WSL2 GPU support has not been qualified.

## Install and dictate

1. Download `FonoSetup.exe` or a full Windows x64 installer from
   [Releases](https://github.com/whitewolf06/Fono/releases/latest).
2. Select a microphone. Download a local model, or configure Fono Server
   with its address, token and installed model.
3. Choose hold-to-talk or press-to-start/press-to-stop.
4. Focus your text field and dictate. Recognition starts after recording ends.

The Windows interface supports direct HTTPS and a managed SSH connection.
Save the connection once; Fono opens the tunnel when needed. An SSH host-key
fingerprint establishes server identity; it is **not** the API token.

The compact overlay keeps a wave and status while processing. If the original
field becomes unavailable, the result remains available to copy.
See the [user guide](docs/user-guide.md) for result actions and settings.

## Other features and privacy

- Optional cleanup, formatting, task/formal styles, translation and custom prompts.
  Basic dictation works without an LLM.
- Application profiles, personal dictionary, snippets, history and speech trainer.
  History and analytics have separate consent controls.
- **“Привет, Лира” voice activation — beta.** Explicit opt-in; wake latency,
  independent voices and long negative runs remain acceptance work.
- Authenticated loopback API and a limited voice-command flow with confirmation.
- Windows Credential Manager for connection keys; local sanitized diagnostics.

Local recognition keeps audio on the PC. Remote recognition sends audio to your
configured server; external LLM processing sends text to the selected provider.
The server panel is not a transcript archive. Experimental live insertion
and the legacy arbitrary-phrase WakeWord setup remain disabled.

Basic local and remote dictation has been checked by the owner. Clean-Windows
installation, the full GPU/device matrix and extended failure/soak scenarios
remain separate acceptance work. Update checks do not install anything;
installation requires a user action after active operations finish.

## Use the API

Run Fono Server from
[`ghcr.io/whitewolf06/fono-server`](https://github.com/whitewolf06/Fono/pkgs/container/fono-server):

- `:0.9.1` or `:0.9.1-vulkan`: Vulkan + CPU.
- `:0.9.1-cuda`: optional CUDA distribution.
- Moving aliases: `:latest`, `:vulkan`, `:cuda`. Pin a version or digest
  for reproducible deployment.

Follow the [server guide](docs/server.md) to configure volumes, models,
credentials, GPU access and protected networking. Open `/panel/` with an admin
token, create an application key and copy its host/token configuration:

```dotenv
FONO_HOST=http://127.0.0.1:17832
FONO_TOKEN=<your-application-key>
```

Use an endpoint reachable from the consumer's runtime. The loopback HTTP example
assumes the same host/network namespace; use TLS or a protected tunnel for
transport protection. Consumers call the server directly without Windows Fono.

```sh
curl --fail-with-body \
  -H "Authorization: Bearer $FONO_TOKEN" \
  -H "Content-Type: audio/wav" \
  --data-binary @speech.wav \
  "$FONO_HOST/v1/recognize?language=ru"
```

[API documentation](docs/api.md) covers discovery, formats, jobs, cancellation,
OpenAI-compatible transcription and errors. Read the service's `/openapi.json`
or `/docs` with a token; `/healthz` is the public readiness probe.

## Develop

Frontend dependencies install without private npm credentials. The exact
WhiteUI **0.6.0** archive is included in
[vendor/whiteui](vendor/whiteui/README.md).

For UI work:

```sh
npm ci
npm run dev:ui
```

Open `http://127.0.0.1:1420/v3.html`. This is a browser mock; it does not run
the microphone, Rust IPC, hotkeys or Windows insertion.

For real Windows CPU development, install the prerequisites in
[development.md](docs/development.md), then run `npm run dev:cpu`.
No CUDA/Vulkan SDK is needed for that path. It uses real native devices
with an isolated debug profile.

## Documentation

- [User guide](docs/user-guide.md), [server deployment](docs/server.md),
  [API](docs/api.md).
- [Architecture](docs/architecture.md), [code rules](docs/code-rules.md),
  [frontend](docs/frontend-architecture.md), [STT](docs/stt.md),
  [remote client](docs/remote-recognizer.md).
- [Development](docs/development.md), [testing](docs/testing.md),
  [delivery and signing](docs/delivery.md), [contributing](CONTRIBUTING.md).

These guides work without a Kvorum account. Project tracking and acceptance
records do not replace the public technical contracts.

Code and documentation use [MIT](LICENSE). The name and graphics have
[separate brand terms](licenses/BRAND.md); third-party components retain their
[own notices](THIRD_PARTY_NOTICES.md).
Report vulnerabilities through [SECURITY.md](SECURITY.md). Keep private audio,
transcripts and credentials out of public issues.
