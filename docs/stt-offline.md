# CPU offline recognition

FONO-115 adds a sherpa offline adapter to the shared `Recognizer` contract.
FONO-116 supplies the shared `fono-audio` pause segmenter and checkpoint merger.
These are backend stages. Settings migration, downloadable package activation
and model-selection UI remain FONO-117; existing persisted settings and model
IPC continue accepting Whisper only until that integration is completed.

## Verified packages

The built-in `src-tauri/src/stt/offline_models.json` records immutable upstream
revisions, per-file sizes and SHA256 digests, supported languages, punctuation,
license sources and the compatible encoder/decoder/joiner/token quartet.
Model weights are downloaded separately; they are not committed or bundled.

| Model                              | Conversion revision                        | Input features | Languages                         | License            |
| ---------------------------------- | ------------------------------------------ | -------------- | --------------------------------- | ------------------ |
| GigaAM v3 `e2e_rnnt`, INT8 encoder | `a6039be7cee829a9044a69ac0ebaf1c191217c97` | 64             | Russian                           | MIT, GigaChat Team |
| Parakeet TDT 0.6B v3 INT8          | `2bda32ec70b097a55adaa07d9a7173915b43cc78` | 80             | 25, including Russian and English | CC-BY-4.0, NVIDIA  |

The [GigaAM punctuation export](https://github.com/k2-fsa/sherpa-onnx/blob/master/scripts/nemo/GigaAM/export-onnx-rnnt-v3-punct.py)
explicitly selects `v3_e2e_rnnt`. Its
[published package](https://huggingface.co/csukuangfj/sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16/tree/a6039be7cee829a9044a69ac0ebaf1c191217c97)
contains the tokenizer and MIT notice. This is the punctuation-capable head,
not an interchange with an unpunctuated CTC/RNNT model.
[Parakeet's model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)
declares its languages, automatic punctuation/case and CC-BY-4.0.
[Sherpa's usage](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html)
documents its three-file INT8 conversion with `model_type=nemo_transducer`.
The [model notices](evidence/licenses/offline-stt-NOTICE.md) retain attribution
and conversion provenance.

## Runtime boundary

The default and `cpu-only` profiles include `offline-stt`. It uses the exact
shared `sherpa-onnx` 1.13.4 dependency already used by speech VAD. No separate
`ort` crate or additional ONNX runtime is loaded. The adapter checks the native
Sherpa version before constructing a model, verifies every pinned artifact,
and checks contiguous tokenizer IDs and the final blank token. Windows read
handles deny writes/deletion until the native context is destroyed.

`SttEngine::ensure_model_loaded` accepts an explicitly supplied package directory
and known `ModelId`; `WorkerPaths.speech_vad` supplies the shared packaged Silero
model. Auto and CPU route these model IDs to CPU. An explicit CUDA/Vulkan choice
fails rather than silently changing the requested acceleration.

Native model results supply text, token timing and the actual CPU backend.
SentencePiece boundaries become Unicode words with absolute sample offsets.
Missing token timestamps remain unavailable. Token end positions use native
durations when present, otherwise the next token boundary/window end. No
detected-language metadata is invented: this wrapper does not return a language
label, although Parakeet selects the spoken language automatically. The
capabilities distinguish automatic language selection from returned detection.
The language argument validates model support; this conversion does not expose
a control that forces Parakeet to decode in one particular language.
Whisper-only prompts and decoding options are rejected for offline models.

## Segmentation and cancellation

The shared segment planner closes ranges in Silero-confirmed pauses (350 ms),
retaining 175 ms of silence after the speech boundary. It keeps consecutive
PCM ranges without skipping the quiet onset, tail or silence. Continuous
speech has a hard 25-second cap. No second microphone or AudioHub is created.
The planner can be reused by future capture segmentation; file analysis keeps
only one VAD frame of pending audio and a list of ranges, without copying PCM.
The existing file-ingest size/duration bounds still govern the input buffer.

Decoding converts at most one 25-second segment to float PCM. Cancellation is
checked between VAD frames, artifact-read blocks and inference segments, and
again after each native decode. A native decode/constructor cannot be aborted
internally. The caller's scheduler permit and update admission remain held
until actual completion; cancelled late text is not published.

The merger appends each completed non-overlapping range once, preserving
intentional repeated words, segment order, timestamps and full input duration.
Scheduler preemption retries only the uncommitted checkpoint. User cancellation
discards the result. A device change between segments is an explicit error.

## Checks and remaining work

Tests cover exact PCM ownership at natural/forced cuts, partial tails,
checkpoint retries, duplicate lexical content, cancellation, package corruption,
Windows file guards, tokenizer timing and CPU-only routing. The explicit
`offline_public_wav_cpu_contract` test accepts `FONO_OFFLINE_TEST_MODELS` pointing
to the two pinned package directories. It checks the existing public FLEURS
`ru-01` and `en-01` fixtures and a long repeated RU recording with pauses.
Ordinary tests never implicitly download or load these models.

Full comparative WER/CER/RTF is FONO-119. Long continuous speech cut inside a
word still needs broader quality qualification; PCM coverage alone is not a
proof of lexical quality at that forced boundary. FONO-117 owns user-facing
selection, download/activation, preferences and complete application metadata.
CUDA/DirectML, microphone/hotkey/overlay/installer and owner acceptance remain
separate checks. Whisper's existing decoding and window policy are unchanged.
