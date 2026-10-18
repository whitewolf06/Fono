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
node scripts/stt-quality.mjs --target src-tauri/target-codex-review/release --download-model --require-no-wer-regression --bandlimited-reconstruction
npm run cache:maintain
```

An existing **explicitly supplied** `--model PATH` is accepted only if its SHA-256
matches the pinned multilingual Whisper tiny model. The runner never scans app
data, reads settings, opens the microphone, inserts text or starts Fono. Downloads
occur only with `--download-model`. By default outputs stay under ignored
`build/stt-quality`; `--report PATH` selects a separate report destination.

## Reconstruction modes and unchanged baseline

Both alternatives receive the same reconstructed 48 kHz input. Legacy uses the
frozen old downsampling algorithm; current uses production `StreamingResampler`.
Two explicit reconstruction modes are retained:

- Without `--bandlimited-reconstruction`, the historical linear 16→48 kHz
  interpolation remains unchanged, including the five-argument example CLI.
- With `--bandlimited-reconstruction`, a cardinal Blackman-windowed sinc uses
  the source Nyquist of 8 kHz and a half-support of 48 source frames. Each phase
  is normalized to unity. Integer source positions return the original PCM16
  sample exactly. This removes the triangular reconstruction's imaging confound
  while keeping **all twenty frozen legacy replay WAV hashes unchanged**.

The corpus, reference text, `baseline.json` and strict threshold are unchanged.
The historical linear results remain negative evidence; the new mode does not
turn those earlier runs into passes. The final Blackman candidate still has
**86 versus 85 word errors/450 on the original linear reconstruction**; all 20
output hashes match the earlier measured wider-Blackman run. Cardinal reconstruction
separately yields 85 versus 85/450. Passing that new methodological comparison
does not pass the original gate. Both modes are derived from 16 kHz fixtures and
**cannot establish quality on native 48 kHz speech**.

At EOF both alternatives receive the same ten-millisecond zero extension and
are trimmed to the frozen round-trip frame count, one 16 kHz sample shorter than
the source fixture. This preserves the finite filter tail without adding silence
to the measured clip. Separate tests cover out-of-band rejection, passband,
fractional phase, callback boundaries, finite zero extension and allocations.

The frozen linear reconstruction also attenuates the source baseband and creates
spectral images. Selecting every third sample in the legacy round trip folds
those images back and nearly recovers the original 16 kHz samples; the new
low-pass removes the images. This instrumental effect is part of the measured
difference. It cannot attribute a WER delta solely to performance on genuine
48 kHz speech and does not waive a failed no-regression criterion. The production
design was specified independently: at 16 kHz output, passband through 7 kHz
within 0.1 dB, stopband from 8 kHz at least 60 dB down, and lookahead no more than
3.1 ms at common capture rates. No inverse-triangle equalization or per-clip
filter selection is used.

The converter also accepts mutually exclusive `--native-input` as its optional
last argument. In that mode an original PCM16 mono WAV must match the declared
44.1/48/96 kHz clock; it is not reconstructed from 16 kHz. Additional native
VoxForge audio and archives remain under ignored `build/perf04/`. Reference
transcripts also appear in the committed numerical JSON reports and retain
GPL-3.0-or-later independently of both these CC BY 4.0 fixtures and the Fono MIT
code. The reports include [original notices and attribution](../../../../docs/evidence/licenses/perf04-data-NOTICE.md)
and the [full GPLv3 text](../../../../docs/evidence/licenses/perf04-voxforge-GPL-3.0.txt). See the
[PERF-04 report](../../../../docs/improvement-perf04.md) for selection,
negative diagnostic results and acceptance limits.

The fixed FLEURS comparison measures the shared **CPU protocol inference engine plus FIR replay**.
It does not run the native VAD gate, Tauri IPC, microphone capture, or Windows
insertion, and its inputs are not native 48 kHz recordings. ARC-09 must later
connect the actual VAD-preprocessed inputs before using these metrics to accept
a VAD change; this baseline is not VAD acceptance.

The JSON report contains micro-averaged WER/CER counts, language results, warmed
RTF, first-inference cold time, model-load time, resampling time, input/output
hashes, hardware, model hash and code hash. Each alternative gets a fresh model;
its first clip is measured cold then repeated before measured warm runs. Here
"cold" means a fresh model/context, not a guarantee that OS file caches are cold.
Timing is observational across machines. The historical CI-03 allowance was
**one percentage point** of WER against frozen baseline and same-run legacy.
`--require-no-wer-regression` adds a method-specific gate with **zero** WER
regression against both. The quality workflow runs two required matrix jobs:
`legacy_linear` uses the original generator, and `cardinal_sinc` explicitly adds
`--bandlimited-reconstruction`. Each gets an isolated runner, distinct run ID and
artifact `stt-quality-cpu-<method>`. `fail-fast: false` lets both jobs finish even
when one gate fails. Neither job ignores build, model-download or quality errors;
the `always()` artifact step retains available reports after failure. A final
check preserves the existing name `quality` and requires the complete matrix to
report `success`; failed, skipped and cancelled results fail closed. A cardinal
pass cannot turn a failed historical linear job green. JSON `acceptance` names
its `reconstruction_policy` and covers that method only, not overall PERF-04
acceptance. CER remains independently reported and may differ even when WER
passes. Removing only `--bandlimited-reconstruction` reruns the original linear
mode; use a separate `--report PATH` to preserve both local results.
That strict original mode is expected to exit 1 for the final candidate. The fresh
native holdout also failed 81 versus 80/341; PERF-04 remains open. A passing
twenty-clip methodological smoke test is not broad model
quality, native microphone acceptance, or evidence of Whisper accuracy in every
recording condition.
