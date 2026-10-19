# Speech model registry

F-01.1 introduces a stable `ModelId { engine, variant }`, separate from a local
file path. Engine identifiers are `whisper`, `giga_am`, and `parakeet`.
Only the six existing Whisper models are downloadable in this stage. Other
engines return an explicit unavailable-engine error until their recognizers
and verified artifacts are integrated in F-01.2/F-01.3.

The catalog includes artifact URL, SHA256, exact byte count, license and its
source, languages, punctuation, and transcription/prompt/language-detection
capabilities. `languages` lists the RU/EN locales exposed by Fono;
`multilingual` records the models' wider upstream language support.
An existing `local_path` reports file presence, not a fresh checksum or runtime
quality certification.

## Settings compatibility

The new persisted selection is:

```json
{
  "stt_model": {
    "id": { "engine": "whisper", "variant": "small" },
    "local_path": "D:\\models\\ggml-small.bin"
  }
}
```

Loading old settings creates this selection from `whisper_model_path` and
preserves the exact path. Unknown/custom Whisper filenames use variant
`custom`; their files are not moved or renamed. The legacy path remains a
compatibility mirror for the existing Whisper runtime and renderer commands.
Legacy path-only writes update both fields in one settings delta. Unrelated
stale preference writes preserve the latest model selection. Unknown engine
identifiers and mismatched known identities/files fail explicitly.

## Model operations

Typed IPC commands are `list_stt_models`, `download_stt_model`, `set_stt_model`,
and `remove_stt_model`. The original Whisper commands remain available.
Downloads stream into a newly created same-directory `.bin.part` file. Exact
size and SHA256 must pass before publication. Atomic hard-link publication
does not overwrite an existing/racing target; the staging link is removed
afterward. Filesystems without hard-link support return an I/O error.
Failed/cancelled transfers remove only their own staging file. Model removal
shares the download admission fence and settings transaction; the selected
model cannot be deleted. Blocking jobs retain update admission until their
actual completion, including when the IPC waiter is dropped.

## Artifact sources and remaining work

The existing [Whisper GGML repository](https://huggingface.co/ggerganov/whisper.cpp/tree/main)
and its LFS pointers supply the hashes and exact sizes. Tiny's size/hash were
also verified against the existing local pinned quality-test artifact.
The repository declares MIT; see the upstream
[Whisper license](https://github.com/openai/whisper/blob/main/LICENSE).
Download URLs retain the existing channel and always enforce the fixed digest.

GigaAM/Parakeet package selection and licenses belong to their upcoming backend
integration; no speculative download is enabled here. Quantized Whisper
catalog entries remain F-01.6. Unit tests exercise migration, concurrent deltas,
publication, checksum/size/cancellation failures, publication races and removal
using small temporary files. Full upstream model transfers and manual desktop
selection are separate checks; this stage does not run microphone inference.
