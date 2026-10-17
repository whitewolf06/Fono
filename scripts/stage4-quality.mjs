// Compare VAD boundaries and decoding independently on the same licensed corpus.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  aggregate,
  compare,
  hash,
  modelSpec,
  validateCorpus,
} from "./stt-quality.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const corpusRoot = path.join(root, "src-tauri/tests/fixtures/stt-quality");
function execute(binary, args) {
  const result = spawnSync(binary, args, {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
    timeout: 600_000,
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(
      `${path.basename(binary)} failed: ${result.stderr.slice(-2_000)}`,
    );
  return JSON.parse(result.stdout);
}

export async function stage4Quality(options = {}) {
  const suffix = process.platform === "win32" ? ".exe" : "";
  const target = path.join(root, "src-tauri/target-codex-review/debug");
  const runner = path.resolve(
    options["--runner"] ?? path.join(target, `fono-stt-quality${suffix}`),
  );
  const trimmer = path.resolve(
    options["--trimmer"] ??
      path.join(target, "examples", `trim-vad-wav${suffix}`),
  );
  const model = path.resolve(
    options["--model"] ?? path.join(root, "build/stt-quality/ggml-tiny.bin"),
  );
  const vadModel = path.join(root, "src-tauri/resources/vad/silero_vad.onnx");
  if (hash(fs.readFileSync(model)) !== modelSpec.sha256)
    throw new Error("Pinned multilingual tiny model required");
  const manifest = JSON.parse(
    fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
  );
  validateCorpus(manifest, corpusRoot);
  const working = path.join(root, "build/stage4-quality");
  fs.mkdirSync(working, { recursive: true });
  const original = manifest.entries.map((entry) => ({
    ...entry,
    path: path.join(corpusRoot, entry.path),
  }));
  const boundaries = [];
  const neural = original.map((entry) => {
    const output = path.join(working, path.basename(entry.path));
    const result = execute(trimmer, [entry.path, output, vadModel]);
    boundaries.push({
      id: entry.id,
      ...result,
      output_sha256: hash(fs.readFileSync(output)),
    });
    return {
      ...entry,
      path: output,
      no_speech: !result.has_speech,
      original_audio_seconds: result.input_samples / 16_000,
    };
  });
  const variants = {};
  for (const [audio, entries] of [
    ["original", original],
    ["neural", neural],
  ]) {
    const corpus = path.join(working, `${audio}.json`);
    fs.writeFileSync(corpus, JSON.stringify({ entries }));
    for (const strategy of ["fast", "accurate"]) {
      console.error(
        `Public corpus: ${audio}/${strategy}, ${entries.length} utterances`,
      );
      const result = execute(runner, [model, corpus, "--strategy", strategy]);
      variants[`${audio}_${strategy}`] = {
        ...result,
        summary: aggregate(result.entries),
        by_language: Object.fromEntries(
          ["ru", "en"].map((language) => [
            language,
            aggregate(
              result.entries.filter((entry) => entry.language === language),
            ),
          ]),
        ),
      };
    }
  }
  const report = {
    schema: 1,
    generated_at: new Date().toISOString(),
    corpus: manifest.corpus,
    license: manifest.license,
    corpus_manifest_sha256: hash(Buffer.from(JSON.stringify(manifest))),
    model: modelSpec,
    vad_model_sha256: hash(fs.readFileSync(vadModel)),
    binaries: {
      runner_sha256: hash(fs.readFileSync(runner)),
      trimmer_sha256: hash(fs.readFileSync(trimmer)),
    },
    hardware: {
      platform: process.platform,
      architecture: os.arch(),
      cpu: os.cpus()[0]?.model,
      logical_cpus: os.cpus().length,
    },
    basis:
      "Original fixed 16 kHz public WAVs. No resampler change/reconstruction; this does not resolve PERF-04's earlier WER regression or native wideband acceptance.",
    default_policy:
      "fast/automatic threads retained; accurate is explicit beam-5 opt-in, without a general quality guarantee",
    boundaries,
    variants,
    comparisons: {
      neural_vs_original_fast: compare(
        variants.original_fast.summary,
        variants.neural_fast.summary,
        0,
      ),
      accurate_vs_fast_original: compare(
        variants.original_fast.summary,
        variants.original_accurate.summary,
        0,
      ),
      accurate_vs_fast_neural: compare(
        variants.neural_fast.summary,
        variants.neural_accurate.summary,
        0,
      ),
    },
  };
  const output = path.resolve(
    options["--report"] ?? path.join(root, "docs/evidence/stage4-quality.json"),
  );
  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.writeFileSync(output, JSON.stringify(report, null, 2) + "\n");
  console.log(
    JSON.stringify({
      report: path.relative(root, output),
      comparisons: report.comparisons,
    }),
  );
  return report;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  const options = {};
  for (let index = 2; index < process.argv.length; index += 2)
    options[process.argv[index]] = process.argv[index + 1];
  stage4Quality(options)
    .then((report) => {
      // Optional beam-search results are evidence, never a changed default.
      // The production fast path may not worsen WER on the fixed corpus.
      if (!report.comparisons.neural_vs_original_fast.passed)
        process.exitCode = 1;
    })
    .catch((error) => {
      console.error(error.message);
      process.exitCode = 1;
    });
}
