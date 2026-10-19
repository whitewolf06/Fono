# Fono

[Русский](README.ru.md) · [Download for Windows x64](https://github.com/whitewolf06/fono/releases/latest) · [Documentation](docs/README.md)

Local voice input for Windows. Dictate into a text field, keep the original
transcript, or apply an optional editing style and translation through your
chosen LLM connection.

![Fono main screen using demonstration data](docs/assets/home-demo.jpg)

## What Fono combines

- Hold-to-talk or press-to-start/press-to-stop, with a floating recording indicator.
- Local Whisper recognition with CPU, CUDA and Vulkan backends. GPU inference
  runs in separate worker processes; the selected mode and actual backend are
  recorded separately.
- CPU and Vulkan ship in the base package. Install the separately signed CUDA
  package from audio settings; Auto uses Vulkan/CPU until it is installed.
- Optional cleanup, formatting, task and formal styles; editable system prompts.
  In toggle mode, change the style or translation in the indicator while recording.
- A local dictionary, session history and a speech trainer for reviewing recurring
  fillers, repetitions and self-corrections in retained transcripts.
- A loopback REST API for transcription jobs and a limited, confirmed voice-command flow.
- Local recognition, separate history/analytics controls, and Windows Credential
  Manager for connection keys. External processing sends text to the selected
  provider; a local LM Studio connection can keep that processing on your machine.

The interface is currently Russian. WakeWord is disabled and experimental live
insertion is hidden. Classic dictation recognizes after recording ends.

<details>
<summary>Processing settings, speech trainer and indicator demo</summary>

![Processing settings in browser mock](docs/assets/processing-demo.jpg)

![Speech trainer using explicitly marked demonstration records](docs/assets/trainer-demo.jpg)

![Recording, transcription and text actions in the indicator preview](docs/assets/overlay-demo.gif)

These are captures of the working Vue interface with built-in demo data. The GIF
shows controlled preview states; frame durations do not measure recognition
latency. No microphone or external Windows field was used. See [asset provenance](docs/assets/README.md).

</details>

## Install

Get the Windows x64 installer from [GitHub Releases](https://github.com/whitewolf06/fono/releases/latest).
Choose a microphone and download a Whisper model in the app. Select CPU for a
first run, then configure a hotkey. Processing and translation are optional.
The [user guide](docs/user-guide.md) explains automatic and manual result actions.

An installed Fono can check the signed update channel manually. Automatic checks
are opt-in; checking does not install an update. Installation is a separate user
action after active work ends. Source changes on a development branch do not
imply that a matching release has been published.

## Build a CPU development copy

Frontend dependencies install anonymously from the public npm registry. The
exact original WhiteUI 0.6.0 tarball is included in [vendor/whiteui](vendor/whiteui/README.md);
its hash and origin are recorded there. No npm token or private package access
is required.

1. On Windows x64, install Node.js 22, the pinned Rust toolchain, MSVC C++ Build
   Tools and Windows SDK, CMake, LLVM/libclang, PowerShell 7 and Python 3.12+.
   WebView2 is needed for the native window; `curl.exe` is used by the bootstrap.
2. Clone `https://github.com/whitewolf06/fono.git` and open its directory.
3. Run `npm ci`; the lockfile includes the local WhiteUI archive.
4. Run `npm run dev:cpu`. It obtains SHA256-pinned speech-VAD resources and
   compiles embedded CPU Whisper; CUDA/Vulkan SDKs and GPU workers are not needed.
5. Download/select a small Whisper model. For the first dictation, select
   Auto or CPU and turn processing and translation off.
6. Open your own text field and test hold/toggle dictation. This is a real native
   app; the default debug profile isolates data and blocks real credentials/autostart.

For UI-only work, run `npm run dev:ui` and open
`http://127.0.0.1:1420/v3.html`. It uses mock ports and demonstration data.
GPU development, profiles and packaging are covered in [development.md](docs/development.md).

## Measured recognition baseline

The historical baseline covers one fixed CPU smoke corpus, not a ranking of
models or GPUs. The following result uses Whisper tiny, 8 threads, debug build,
an Intel i5-12600KF, and 20 public RU/EN FLEURS utterances (235.36 seconds, 450 words).

| Model / backend                        | Warm inference time | Real-time factor |    WER |
| -------------------------------------- | ------------------: | ---------------: | -----: |
| Whisper tiny / CPU, current FIR replay |       25.19 s total |            0.107 | 19.56% |

RTF is inference seconds per audio second. It excludes model loading, VAD,
resampling, LLM processing and insertion. Inputs were reconstructed from 16 to
48 kHz and downsampled again; they are not native wideband recordings. Other
accelerators and end-to-end latency need separate measurements.

The strict resampler no-regression criterion is still open: the same-run legacy
baseline has WER 18.89%, a difference of +0.67 percentage points. See the
[full evidence](docs/evidence/stage2-stt-quality-final.json) and
[measurement limits](docs/improvement-stage-2.md).

## CPU engine comparison

FONO-119 uses the same pinned public FLEURS corpus, production PCM preparation
and 25-second segment boundaries on one Intel i5-12600KF. This is one sequential
native **debug** run: Whisper fast / 8 threads; GigaAM and Parakeet greedy / 2
threads, matching their current Fono configurations. The historical resampler
baseline above is a separate experiment and is not the comparison baseline.

| Model                       | Language / utterances |         WER |    CER | Warm decode |   RTF |
| --------------------------- | --------------------- | ----------: | -----: | ----------: | ----: |
| GigaAM v3 e2e RNNT INT8     | RU / 10               |       1.95% |  0.34% |     7.871 s | 0.059 |
| GigaAM v3 e2e RNNT INT8     | EN                    | unsupported |      — |           — |     — |
| Parakeet TDT 0.6B v3 INT8   | RU / 10               |      11.22% |  4.31% |    11.983 s | 0.090 |
| Parakeet TDT 0.6B v3 INT8   | EN / 10               |      15.51% | 12.65% |     8.702 s | 0.085 |
| Whisper Large v3 Turbo FP16 | RU / EN               |  incomplete |      — |           — |     — |

RU contains 133.48 audio seconds and 205 reference words; EN contains 101.88
seconds and 245 words. Unsupported English is excluded from GigaAM's denominator.
RTF includes recognition and segment merging, with file decoding, VAD, model
loading, LLM and insertion measured separately or excluded.

| Model                     | Model load | First RU decode | Mean warm RU utterance |
| ------------------------- | ---------: | --------------: | ---------------------: |
| GigaAM v3 e2e RNNT INT8   |    4.294 s |         1.324 s |                0.787 s |
| Parakeet TDT 0.6B v3 INT8 |   11.868 s |         1.402 s |                1.198 s |

These numbers describe this small corpus and machine. They do not establish a
general model ranking, release latency, GPU performance or personal-voice
acceptance. The complete Whisper Turbo CPU series remains pending; partial
results are not mixed into the table or UI. FLEURS is by The Google and
HuggingFace Datasets Authors (2022), licensed CC-BY-4.0.

[Reproduction, limits and remaining work](docs/improvement-fono119.md) ·
[raw results, model hashes and hardware](docs/evidence/fono119-comparison.json).

## Documentation and contributions

- [Documentation index](docs/README.md), [architecture](docs/architecture.md), [ADRs](docs/adr/README.md).
- [Contributing](CONTRIBUTING.md), [checks](docs/testing.md), [current status and acceptance](docs/STATUS.md).
- [REST API](docs/LOCAL_TRANSCRIPTION_API.md), [privacy and user workflow](docs/user-guide.md).
- [Security reporting](SECURITY.md). Keep private audio, transcripts and credentials out of public issues.

Fono code and documentation use [MIT](LICENSE). The name and graphics have
[separate brand terms](docs/brand-assets.md); dependencies and models retain
their own terms in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
