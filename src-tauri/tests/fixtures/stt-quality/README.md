# Fixed public speech quality smoke corpus

Twenty real test utterances from **FLEURS** (Google and HuggingFace Datasets
Authors, 2022), ten English and ten Russian. Dataset revision:
`437d9e667fa3a2a08a1957f810bf2c3b5050d221`.

Source and license:

- [Pinned dataset card](https://huggingface.co/datasets/google/fleurs/blob/437d9e667fa3a2a08a1957f810bf2c3b5050d221/README.md), which declares CC BY 4.0.
- [FLEURS paper](https://arxiv.org/abs/2205.12446).
- [CC BY 4.0 legal code](https://creativecommons.org/licenses/by/4.0/legalcode.en), copied in `LICENSE-CC-BY-4.0.txt`.

The manifest records every archive URL, member, original filename, sentence ID,
transcription, original-byte SHA-256 and resulting PCM16 WAV SHA-256. These audio
files and transcriptions retain **CC BY 4.0**, independently of the Fono MIT code
license. Credit the source and indicate these changes when redistributing them.

Selection is the first ten WAV members of each pinned test archive, before any
recognition was run. Originals are converted to mono PCM16 at 16 kHz. Per language,
the fourth clip is attenuated by 18 dB, the eighth has deterministic white noise
at 20 dB RMS SNR, and the tenth repeats the same genuine utterance after a one-second
pause (its reference repeats too). No speech is generated and no owner's recording
is included. These transformations are described by `condition` in the manifest.

Regenerate with Python 3.12, standard library only:

```powershell
python scripts/prepare-stt-quality-corpus.py
npx prettier --write src-tauri/tests/fixtures/stt-quality/manifest.json
node --test scripts/stt-quality.test.mjs
```

The preparation streams each public archive and closes the connection after ten
matching members; it does not download whole language datasets.

## Running the CPU comparison

```powershell
$env:CARGO_TARGET_DIR = "$PWD/src-tauri/target-codex-review"
cargo build --manifest-path src-tauri/Cargo.toml --locked --release --no-default-features -p fono-audio --example resample-wav -p fono-stt-worker --bin fono-stt-quality
node scripts/stt-quality.mjs --target src-tauri/target-codex-review/release --download-model
```

An existing **explicitly supplied** `--model PATH` is accepted only if its SHA-256
matches the pinned multilingual Whisper tiny model. The runner never scans app
data, reads settings, opens the microphone, inserts text or starts Fono. Downloads
occur only with `--download-model`. Outputs stay under ignored `build/stt-quality`.

Both alternatives reconstruct identical 48 kHz input from the 16 kHz fixtures
using the frozen old interpolation. The legacy alternative downsamples with that
same old algorithm; current uses the production `StreamingResampler`. This derived
replay is useful for a controlled regression comparison, and **cannot establish
quality on native 48 kHz speech**. The separate spectral test covers out-of-band
alias rejection. At EOF both alternatives receive the same ten-millisecond zero
extension and are trimmed to the frozen legacy round-trip frame count, one
16 kHz sample shorter than the source fixture. This preserves a delayed
filter's tail without adding silence to the measured clip. Frozen legacy replay
hashes ensure this boundary handling does not change the historical baseline.
No samples or filter parameters are selected based on measured WER.

The frozen linear reconstruction also attenuates the source baseband and creates
spectral images. Selecting every third sample in the legacy round trip folds
those images back and nearly recovers the original 16 kHz samples; the new
low-pass removes the images. This instrumental effect is part of the measured
difference. It cannot attribute a WER delta solely to performance on genuine
48 kHz speech, and it does not waive a failed no-regression criterion. A future
filter design needs an independent passband/stopband/latency specification and
native wideband recordings rather than tuning to these twenty transcripts.

The measured path is the shared **CPU protocol inference engine plus FIR replay**.
It does not run the native VAD gate, Tauri IPC, microphone capture, or Windows
insertion, and its inputs are not native 48 kHz recordings. ARC-09 must later
connect the actual VAD-preprocessed inputs before using these metrics to accept
a VAD change; this baseline is not VAD acceptance.

The JSON report contains micro-averaged WER/CER counts, language results, warmed
RTF, first-inference cold time, model-load time, resampling time, input/output
hashes, hardware, model hash and code hash. Each alternative gets a fresh model;
its first clip is measured cold then repeated before measured warm runs. Here
"cold" means a fresh model/context, not a guarantee that OS file caches are cold.
Timing is observational across machines. WER regression greater than **one
percentage point** against either the frozen historical baseline or same-run
legacy resampling fails CI. A passing twenty-clip smoke test is not broad model
quality, native microphone acceptance, or evidence of Whisper accuracy in every
recording condition.
