import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const corpusRoot = path.join(root, "src-tauri/tests/fixtures/stt-quality");
export const modelSpec = {
  basename: "ggml-tiny.bin",
  revision: "5359861c739e955e79d9a303bcbc70fb988958b1",
  sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
};
export function hash(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}
export function validateCorpus(manifest, directory) {
  if (
    manifest.schema !== 1 ||
    manifest.license !== "CC-BY-4.0" ||
    manifest.entries.length !== 20
  )
    throw new Error("Unexpected fixed corpus contract");
  const ids = new Set();
  for (const entry of manifest.entries) {
    if (
      ids.has(entry.id) ||
      path.basename(entry.path) !== entry.path ||
      !["ru", "en"].includes(entry.language)
    )
      throw new Error("Invalid corpus entry");
    ids.add(entry.id);
    const bytes = fs.readFileSync(path.join(directory, entry.path));
    if (hash(bytes) !== entry.sha256)
      throw new Error(`Corpus hash mismatch: ${entry.id}`);
    if (
      bytes.toString("ascii", 0, 4) !== "RIFF" ||
      bytes.readUInt32LE(24) !== 16000 ||
      bytes.readUInt16LE(34) !== 16
    )
      throw new Error(`Corpus format mismatch: ${entry.id}`);
  }
  for (const language of ["ru", "en"])
    if (
      manifest.entries.filter((entry) => entry.language === language).length !==
      10
    )
      throw new Error("Corpus must contain ten utterances per language");
}
export function aggregate(entries) {
  const sums = {
    word_errors: 0,
    reference_words: 0,
    character_errors: 0,
    reference_characters: 0,
    warm_seconds: 0,
    audio_seconds: 0,
  };
  for (const entry of entries) {
    for (const key of [
      "word_errors",
      "reference_words",
      "character_errors",
      "reference_characters",
    ])
      sums[key] += entry.errors[key];
    sums.warm_seconds += entry.warm_seconds;
    sums.audio_seconds += entry.audio_seconds;
  }
  if (
    !sums.reference_words ||
    !sums.reference_characters ||
    !sums.audio_seconds
  )
    throw new Error("Quality report has no reference denominator");
  return {
    ...sums,
    wer: sums.word_errors / sums.reference_words,
    cer: sums.character_errors / sums.reference_characters,
    rtf: sums.warm_seconds / sums.audio_seconds,
  };
}
export function compare(baseline, candidate, maximumRegression = 0.01) {
  const werDelta = candidate.wer - baseline.wer;
  return {
    maximum_wer_regression: maximumRegression,
    wer_delta: werDelta,
    cer_delta: candidate.cer - baseline.cer,
    no_wer_regression: werDelta <= 1e-12,
    passed: werDelta <= maximumRegression + 1e-12,
  };
}
function execute(binary, arguments_) {
  const result = spawnSync(binary, arguments_, {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(
      `${path.basename(binary)} failed: ${result.stderr.slice(-4000)}`,
    );
  return JSON.parse(result.stdout);
}
async function run() {
  const options = {};
  for (let index = 2; index < process.argv.length; index++) {
    const key = process.argv[index];
    if (key === "--download-model") options[key] = true;
    else options[key] = process.argv[++index];
  }
  const suffix = process.platform === "win32" ? ".exe" : "";
  const target = path.resolve(
    options["--target"] ??
      path.join(root, "src-tauri/target-codex-review/debug"),
  );
  const runner =
    options["--runner"] ?? path.join(target, `fono-stt-quality${suffix}`);
  const resampler =
    options["--resampler"] ??
    path.join(target, "examples", `resample-wav${suffix}`);
  const working = path.join(root, "build/stt-quality");
  fs.mkdirSync(working, { recursive: true });
  const model = path.resolve(
    options["--model"] ?? path.join(working, modelSpec.basename),
  );
  if (!fs.existsSync(model) && options["--download-model"]) {
    const url = `https://huggingface.co/ggerganov/whisper.cpp/resolve/${modelSpec.revision}/${modelSpec.basename}`;
    const response = await fetch(url);
    if (!response.ok)
      throw new Error(`Public model download failed: ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    if (hash(bytes) !== modelSpec.sha256)
      throw new Error("Downloaded model hash mismatch");
    fs.writeFileSync(model, bytes);
  }
  const modelHash = hash(fs.readFileSync(model));
  if (modelHash !== modelSpec.sha256)
    throw new Error(
      "Quality baseline requires the pinned multilingual tiny model",
    );
  const manifestBytes = fs.readFileSync(path.join(corpusRoot, "manifest.json"));
  const manifest = JSON.parse(manifestBytes);
  validateCorpus(manifest, corpusRoot);
  const alternatives = {};
  for (const mode of ["legacy", "current"]) {
    const directory = path.join(working, mode);
    fs.mkdirSync(directory, { recursive: true });
    let resampleSeconds = 0;
    const entries = manifest.entries.map((entry) => {
      const output = path.join(directory, entry.path);
      const converted = execute(resampler, [
        path.join(corpusRoot, entry.path),
        output,
        mode,
        "48000",
      ]);
      resampleSeconds += converted.resample_seconds;
      return {
        ...entry,
        path: output,
        replay_sha256: hash(fs.readFileSync(output)),
      };
    });
    const input = path.join(directory, "corpus.json");
    fs.writeFileSync(input, JSON.stringify({ entries }));
    console.error(`Running CPU quality: ${mode}, ${entries.length} utterances`);
    const result = execute(runner, [model, input]);
    alternatives[mode] = {
      ...result,
      resample_seconds: resampleSeconds,
      summary: aggregate(result.entries),
      by_language: Object.fromEntries(
        ["ru", "en"].map((language) => [
          language,
          aggregate(
            result.entries.filter((entry) => entry.language === language),
          ),
        ]),
      ),
      replay_hashes: entries.map((entry) => ({
        id: entry.id,
        sha256: entry.replay_sha256,
      })),
    };
  }
  const source = path.join(
    root,
    "src-tauri/crates/fono-audio/src/audio_source/resample.rs",
  );
  const revision = spawnSync("git", ["rev-parse", "HEAD"], {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
  }).stdout?.trim();
  const manifestHash = hash(Buffer.from(JSON.stringify(manifest)));
  const baselinePath = path.join(corpusRoot, "baseline.json");
  let historicalComparison = null;
  if (fs.existsSync(baselinePath)) {
    const baseline = JSON.parse(fs.readFileSync(baselinePath));
    if (
      baseline.model.sha256 !== modelHash ||
      baseline.corpus_manifest_sha256 !== manifestHash
    )
      throw new Error("Historical baseline model/corpus identity mismatch");
    if (
      JSON.stringify(baseline.replay_hashes) !==
      JSON.stringify(alternatives.legacy.replay_hashes)
    )
      throw new Error(
        "Frozen legacy replay changed; refusing to replace the baseline silently",
      );
    historicalComparison = compare(
      baseline.summary,
      alternatives.current.summary,
    );
  }
  const report = {
    schema: 1,
    generated_at: new Date().toISOString(),
    source_revision: revision,
    runner_binary_sha256: hash(fs.readFileSync(runner)),
    replay_binary_sha256: hash(fs.readFileSync(resampler)),
    profile: path.basename(target),
    resampler_source_sha256: hash(fs.readFileSync(source)),
    corpus: manifest.corpus,
    corpus_manifest_sha256: manifestHash,
    model: {
      basename: path.basename(model),
      sha256: modelHash,
      revision: modelSpec.revision,
    },
    hardware: {
      platform: process.platform,
      architecture: os.arch(),
      cpu: os.cpus()[0]?.model,
      logical_cpus: os.cpus().length,
    },
    replay:
      "16 kHz public speech reconstructed with frozen linear interpolation to 48 kHz, then converted to 16 kHz. This is not native wideband audio.",
    timing:
      "Each alternative loads a fresh CPU model; first utterance is measured cold, repeated for warmup; aggregate contains warmed runs. No hard performance gate across machines.",
    alternatives,
    comparison: compare(
      alternatives.legacy.summary,
      alternatives.current.summary,
    ),
    historical_comparison: historicalComparison,
  };
  const output = path.resolve(
    options["--report"] ?? path.join(working, "report.json"),
  );
  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.writeFileSync(output, JSON.stringify(report, null, 2) + "\n");
  console.log(
    JSON.stringify({
      report: path.relative(root, output),
      ...report.comparison,
      baseline_wer: alternatives.legacy.summary.wer,
      candidate_wer: alternatives.current.summary.wer,
    }),
  );
  if (!report.comparison.passed || historicalComparison?.passed === false)
    process.exitCode = 1;
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
)
  run().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
