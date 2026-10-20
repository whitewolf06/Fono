// FONO-118: one matched FP16/quantized replay of the fixed CI-03 public corpus.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import readline from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";
import { aggregate, compare, hash, validateCorpus } from "./stt-quality.mjs";
import { score } from "./stage5-quality.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = path.join(
  root,
  "src-tauri/resources/whisper-quantized-models.json",
);
const corpusRoot = path.join(root, "src-tauri/tests/fixtures/stt-quality");
const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
async function fileHash(file) {
  const digest = createHash("sha256");
  for await (const chunk of fs.createReadStream(file)) digest.update(chunk);
  return digest.digest("hex");
}
export function verifyRun(run, corpus) {
  if (
    run.backend !== "cpu" ||
    run.threads !== 8 ||
    run.decoding?.strategy !== "fast" ||
    run.entries?.length !== 20
  )
    throw new Error("Unexpected CPU run contract");
  const ids = new Set();
  for (const entry of run.entries) {
    const source = corpus.entries.find((item) => item.id === entry.id);
    if (!source || ids.has(entry.id) || entry.language !== source.language)
      throw new Error("Quality corpus identity mismatch");
    ids.add(entry.id);
    const expected = score(source.reference, entry.text);
    for (const key of [
      "word_errors",
      "reference_words",
      "character_errors",
      "reference_characters",
    ])
      if (expected[key] !== entry.errors[key])
        throw new Error(`Scorer mismatch: ${entry.id}/${key}`);
    for (const key of ["warm_seconds", "audio_seconds"])
      if (!Number.isFinite(entry[key]) || entry[key] <= 0)
        throw new Error("Invalid timing");
    if (
      Math.abs(entry.audio_seconds - source.samples / source.sample_rate) >
      0.001
    )
      throw new Error("Audio duration mismatch");
  }
  return Object.fromEntries(
    ["all", "ru", "en"].map((language) => [
      language,
      aggregate(
        run.entries.filter(
          (entry) => language === "all" || entry.language === language,
        ),
      ),
    ]),
  );
}
export function comparisons(baseline, quantized) {
  return Object.fromEntries(
    ["all", "ru", "en"].map((language) => [
      language,
      compare(baseline[language], quantized[language]),
    ]),
  );
}
function pcm(file) {
  const bytes = fs.readFileSync(file);
  for (let offset = 12; offset + 8 <= bytes.length;) {
    const size = bytes.readUInt32LE(offset + 4);
    if (bytes.toString("ascii", offset, offset + 4) === "data")
      return bytes.subarray(offset + 8, offset + 8 + size).toString("base64");
    offset += 8 + size + (size & 1);
  }
  throw new Error("WAV data missing");
}
async function workerProbe(worker, model, entry, log) {
  const fd = fs.openSync(log, "w");
  const child = spawn(worker, [], {
    stdio: ["pipe", "pipe", fd],
    windowsHide: true,
  });
  const lines = readline.createInterface({ input: child.stdout });
  const iterator = lines[Symbol.asyncIterator]();
  let sequence = 0;
  try {
    async function request(type, payload = {}) {
      const request_id = `quantized-${++sequence}`;
      const started = performance.now();
      child.stdin.write(
        JSON.stringify({
          type,
          protocol_version: 4,
          request_id,
          operation_id: "public-quantized-compatibility",
          ...payload,
        }) + "\n",
      );
      let timeout;
      try {
        const response = await Promise.race([
          iterator.next(),
          new Promise((_, reject) => {
            timeout = setTimeout(
              () => reject(new Error(`Worker timeout: ${type}`)),
              300000,
            );
          }),
          new Promise((_, reject) => child.once("error", reject)),
        ]);
        if (response.done) throw new Error("Worker EOF");
        const value = JSON.parse(response.value);
        if (
          value.protocol_version !== 4 ||
          value.request_id !== request_id ||
          value.type === "error"
        )
          throw new Error(
            `Worker rejected ${type}: ${value.type}/${value.code || "envelope"}`,
          );
        return {
          ...value,
          roundtrip_seconds: (performance.now() - started) / 1000,
        };
      } finally {
        clearTimeout(timeout);
      }
    }
    const ready = await request("hello");
    if (
      ready.type !== "ready" ||
      ready.backend !== "vulkan" ||
      !ready.capabilities.supports_decoding_options
    )
      throw new Error("Expected protocol-4 Vulkan worker");
    const load = await request("load", { model_path: model });
    if (load.type !== "model_loaded")
      throw new Error("Worker model load failed");
    const result = await request("transcribe", {
      model_path: model,
      language: entry.language,
      samples_i16_base64: pcm(entry.path),
      decoding: { strategy: "fast", threads: 8 },
    });
    if (
      result.type !== "result" ||
      result.backend !== "vulkan" ||
      !result.text?.trim()
    )
      throw new Error("Worker transcription failed");
    return {
      scope: "one public WAV; compatibility only",
      ready,
      load,
      result,
      errors: score(entry.reference, result.text),
    };
  } finally {
    lines.close();
    child.kill();
    await new Promise((resolve) =>
      child.exitCode !== null ? resolve() : child.once("exit", resolve),
    );
    fs.closeSync(fd);
  }
}
async function main(args) {
  const compatibilityOnly = args.at(-1) === "--compatibility-only";
  if (compatibilityOnly) args = args.slice(0, -1);
  const [runnerInput, modelsInput, reportInput, workerInput] = args;
  if (
    !runnerInput ||
    !modelsInput ||
    !reportInput ||
    args.length > 4 ||
    (compatibilityOnly && !workerInput)
  )
    throw new Error(
      "usage: node scripts/stt-quantized-quality.mjs RUNNER MODELS_DIR REPORT [VULKAN_WORKER] [--compatibility-only]",
    );
  const runner = path.resolve(runnerInput),
    models = path.resolve(modelsInput),
    reportPath = path.resolve(reportInput);
  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  const manifest = read(manifestPath),
    corpusFile = path.join(corpusRoot, "manifest.json"),
    corpus = read(corpusFile);
  validateCorpus(corpus, corpusRoot);
  const prepared = {
    entries: corpus.entries.map((entry) => ({
      ...entry,
      path: path.join(corpusRoot, entry.path),
    })),
  };
  const input = reportPath + ".input.json";
  fs.writeFileSync(input, JSON.stringify(prepared));
  const report = {
    schema: 1,
    task: "FONO-118",
    started_at: new Date().toISOString(),
    source_revision: spawnSync("git", ["rev-parse", "HEAD"], {
      cwd: root,
      encoding: "utf8",
    }).stdout.trim(),
    registry_sha256: hash(fs.readFileSync(manifestPath)),
    corpus_sha256: hash(fs.readFileSync(corpusFile)),
    runner_sha256: await fileHash(runner),
    hardware: {
      os: os.release(),
      arch: os.arch(),
      cpu: os.cpus()[0]?.model,
      logical_cpus: os.cpus().length,
      memory_bytes: os.totalmem(),
    },
    policy: {
      backend: "cpu",
      strategy: "fast",
      threads: 8,
      corpus: corpus.corpus,
      samples: 20,
      replay: "original 16 kHz PCM; no resampler or DSP change",
      maximum_wer_regression: 0.01,
      limitations:
        "One run on a small public corpus; timing is indicative. No owner microphone, CUDA, sustained load or general quality guarantee.",
    },
    cpu_quality_status: compatibilityOnly ? "not_run" : "pending",
    runs: {},
    comparisons: {},
    worker: workerInput
      ? { sha256: await fileHash(path.resolve(workerInput)), probes: {} }
      : null,
  };
  const save = () =>
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2) + "\n");
  save();
  for (const artifact of compatibilityOnly
    ? []
    : [...manifest.baselines, ...manifest.models]) {
    const model = path.join(models, artifact.filename);
    if (
      fs.statSync(model).size !== artifact.bytes ||
      (await fileHash(model)) !== artifact.sha256
    )
      throw new Error(`Model hash/size mismatch: ${artifact.variant}`);
    console.log("CPU replay", artifact.variant);
    const fd = fs.openSync(reportPath + `.${artifact.variant}.log`, "w");
    let execution;
    try {
      execution = spawnSync(
        runner,
        [model, input, "--strategy", "fast", "--threads", "8"],
        {
          encoding: "utf8",
          timeout: 1800000,
          maxBuffer: 16 * 1024 * 1024,
          stdio: ["ignore", "pipe", fd],
          windowsHide: true,
        },
      );
    } finally {
      fs.closeSync(fd);
    }
    if (execution.error || execution.status !== 0)
      throw (
        execution.error ||
        new Error(`Runner exited ${execution.status}: ${artifact.variant}`)
      );
    const raw = JSON.parse(execution.stdout),
      summary = verifyRun(raw, corpus);
    report.runs[artifact.variant] = { artifact, summary, raw };
    if (artifact.baseline)
      report.comparisons[artifact.variant] = comparisons(
        report.runs[artifact.baseline].summary,
        summary,
      );
    console.log(artifact.variant, JSON.stringify(summary.all));
    save();
  }
  if (workerInput)
    for (const artifact of manifest.models) {
      const model = path.join(models, artifact.filename);
      if (
        fs.statSync(model).size !== artifact.bytes ||
        (await fileHash(model)) !== artifact.sha256
      )
        throw new Error(`Model hash/size mismatch: ${artifact.variant}`);
      console.log("Vulkan compatibility", artifact.variant);
      report.worker.probes[artifact.variant] = await workerProbe(
        path.resolve(workerInput),
        model,
        prepared.entries[0],
        reportPath + `.${artifact.variant}.vulkan.log`,
      );
      save();
    }
  report.completed_at = new Date().toISOString();
  report.cpu_quality_status = compatibilityOnly ? "not_run" : "completed";
  report.quality_passed = compatibilityOnly
    ? null
    : Object.values(report.comparisons).every((comparison) =>
        Object.values(comparison).every((language) => language.passed),
      );
  save();
  if (report.quality_passed === false) process.exitCode = 1;
}
if (
  process.argv[1] &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
)
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
