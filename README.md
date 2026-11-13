# Fono

[Русский](README.ru.md) · [Download for Windows x64](https://github.com/whitewolf06/Fono/releases/latest) · [Documentation](docs/README.md)

Voice input for Windows with local recognition or your own Fono Server on
Ubuntu. Dictate into a text field with a hotkey and optionally edit or translate
the result through your chosen LLM connection. The interface supports Russian
and English.

This repository contains both **Fono Desktop** and the headless **Fono Server**.
The [0.9.0 release candidate](docs/releases/0.9.0.md) brings the server API,
web panel and Windows remote recognition together. Published installers are
listed in [GitHub Releases](https://github.com/whitewolf06/Fono/releases).

## Features

- Hold-to-talk or press-to-start/press-to-stop, with a floating recording indicator.
- Local Whisper recognition on CPU, Vulkan or CUDA; offline GigaAM and Parakeet
  recognition on CPU. Models are downloaded separately in the app.
- Remote recognition through Fono Server, with connection settings, server-model
  selection and an optional managed SSH tunnel in the Windows interface.
  Server mode does not load local speech-model weights; a local CPU reserve is
  used only when explicitly configured or selected after a failure.
- CPU and Vulkan in the base installer; a separately signed CUDA package
  installed explicitly from audio settings. Auto uses Vulkan on a suitable
  discrete GPU, or CPU when none is available; CUDA is selected manually.
  Fono Server has separate Vulkan and CUDA Docker distributions; see
  [the GPU distribution decision](docs/adr/0006-vulkan-default-optional-cuda.md).
- **“Привет, Лира” voice activation — beta.** Enable it explicitly and wait for
  the ready sound or indicator before dictating. Wake latency and recognition
  across different voices still need improvement.
- Optional text cleanup, formatting, task and formal styles, translation, and
  editable prompts. Basic dictation works without an LLM connection.
- Application profiles, a personal dictionary, session history and a speech
  trainer. History and analytics have separate controls.
- A loopback REST API for transcription jobs and a limited, confirmed
  voice-command flow.
- Windows Credential Manager for connection keys. Local recognition keeps audio
  on the PC; remote recognition sends audio to your configured Fono Server.
  External LLM processing sends text to the selected provider.

Classic dictation recognizes after recording ends. Experimental live insertion
and the older custom-phrase WakeWord setup remain unavailable. Lira beta is a
separate, fixed-phrase implementation; see [its scope and limits](docs/wake-lira.md).

## Install and start

1. Download `FonoSetup.exe` or a full Windows x64 installer from
   [GitHub Releases](https://github.com/whitewolf06/Fono/releases/latest).
2. Select a microphone and a recognition source: download a local model, or
   connect to Fono Server in the app. Server mode needs an API token and an
   installed server model; no local speech model is required.
3. Configure a hotkey and try a first dictation with text processing off.
4. Open your target field, record, and release the hotkey or press it again,
   depending on the selected mode.

The [user guide](docs/user-guide.md) covers result actions, settings, Lira,
history, privacy and updates. Secondary settings are grouped under “Advanced”.

Update checks do not install anything. Automatic checks are opt-in; installation
is a separate user action after active work ends. A source version does not
necessarily have a corresponding published installer.

## Fono Server on Ubuntu

Fono Server runs without Tauri, a microphone or a desktop session. Its CUDA
Docker image supports GPU Whisper and CPU recognition. The embedded web panel
provides token login, status, models, queues, API documentation and a trial
audio-file request. Administrators can create and revoke a separate recognition
key for each application and copy its host/token configuration.

Start with [deployment and Docker configuration](docs/server.md), then
[the panel](docs/server-panel.md) and [application keys](docs/server-clients.md).
Windows connects through HTTPS or its managed SSH connection. Applications on
the server can call the API directly over a reachable protected endpoint;
Windows does not need to relay their requests.

## Development

Frontend dependencies install from public npm. The exact WhiteUI 0.6.0 archive
is included in [vendor/whiteui](vendor/whiteui/README.md); no npm token is needed.

For a Windows CPU development copy, install Node.js 22, the pinned Rust toolchain,
MSVC C++ Build Tools and Windows SDK, CMake, LLVM/libclang, PowerShell 7, Python
3.12+ and WebView2, then run:

```powershell
npm ci
npm run dev:cpu
```

The launcher obtains hash-pinned native resources and uses an isolated debug
profile. It runs real dictation; its test profile blocks real credentials and
autostart. CUDA/Vulkan SDKs are not needed for this CPU path.

For UI work, use `npm run dev:ui` and open
`http://127.0.0.1:1420/v3.html`. This browser view uses mock ports and demo data.
[Development and packaging](docs/development.md) describes native dev, GPU
workers and the separate Lira release resources.

## Project documentation

- [Documentation index](docs/README.md), [user guide](docs/user-guide.md).
- [Architecture](docs/architecture.md), [ADRs](docs/adr/README.md).
- [Contributing](CONTRIBUTING.md), [checks and CI](docs/testing.md).
- [Ubuntu API server](docs/server.md), [web panel](docs/server-panel.md),
  [application keys](docs/server-clients.md), [server API v2](docs/server-api.md).
- [Windows server connection](docs/user-guide.md#fono-server-распознавание-на-сервере),
  [remote recognition contract](docs/remote-recognizer.md).
- [Current status](docs/STATUS.md), [remaining work](docs/roadmap.md).
- [Historical UI captures](docs/assets/README.md), with dates and demo provenance.
- [Historical implementation reports and measurements](docs/archive/README.md).
  Measurements describe their recorded models, corpus, hardware and revision.
- [Security reporting](SECURITY.md). Keep private audio, transcripts and
  credentials out of public issues.

Code and documentation use [MIT](LICENSE). The name and graphics have
[separate brand terms](docs/brand-assets.md); dependencies and models retain
their own terms in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
