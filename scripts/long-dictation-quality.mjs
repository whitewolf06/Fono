// FONO-124: one bounded paired replay; no downloads, parameter search or user audio.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { hash, modelSpec, validateCorpus } from "./stt-quality.mjs";
import { score } from "./stage5-quality.mjs";
import { qualityLineage } from "./stt-quality-lineage.mjs";
import { RUNTIME_DLLS } from "./native-test-runtime.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const corpusRoot = path.join(root, "src-tauri/tests/fixtures/stt-quality");
const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
export function pcm(bytes) {
  if (
    bytes.toString("ascii", 0, 4) !== "RIFF" ||
    bytes.toString("ascii", 8, 12) !== "WAVE"
  )
    throw Error("Expected RIFF WAV");
  let format, data;
  for (let offset = 12; offset + 8 <= bytes.length;) {
    const size = bytes.readUInt32LE(offset + 4);
    const end = offset + 8 + size;
    if (end > bytes.length) throw Error("Truncated WAV");
    const name = bytes.toString("ascii", offset, offset + 4);
    if (name === "fmt ") format = bytes.subarray(offset + 8, end);
    if (name === "data") data = bytes.subarray(offset + 8, end);
    offset = end + (size % 2);
  }
  if (
    !format ||
    format.length < 16 ||
    format.readUInt16LE(0) !== 1 ||
    format.readUInt16LE(2) !== 1 ||
    format.readUInt32LE(4) !== 16000 ||
    format.readUInt16LE(14) !== 16 ||
    !data ||
    data.length % 2
  )
    throw Error("Expected 16 kHz mono PCM16");
  return data;
}
function wav(data) {
  const header = Buffer.alloc(44);
  header.write("RIFF");
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVEfmt ", 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(16000, 24);
  header.writeUInt32LE(32000, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36);
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}
// Symphonia PCM16 -> f32 and production float_to_i16, with Rust's tie rounding.
// Keep the WAV's original hash separately; strong peaks can change by one LSB.
export function decodedPcmHash(data) {
  const decoded = Buffer.alloc(data.length);
  for (let offset = 0; offset < data.length; offset += 2) {
    const sample = Math.fround(
      Math.fround(data.readInt16LE(offset) / 32768) * 32767,
    );
    decoded.writeInt16LE(
      Math.sign(sample) * Math.floor(Math.abs(sample) + 0.5),
      offset,
    );
  }
  return hash(decoded);
}
export function derive(corpus, directory) {
  const entries = [];
  for (const language of ["ru", "en"]) {
    const selected = [];
    let total = 0;
    // Whole consecutive utterances in original order; never select by quality.
    for (const entry of corpus.entries.filter(
      (entry) => entry.language === language,
    )) {
      if (
        total + entry.samples + (selected.length + 1) * 16000 + 1600 >
        60 * 16000
      )
        break;
      selected.push(entry);
      total += entry.samples;
    }
    if (selected.length < 2) throw Error("Insufficient whole utterances");
    const audio = Buffer.alloc(60 * 16000 * 2);
    const padding = 60 * 16000 - total - 1600;
    const gap = Math.floor(padding / selected.length);
    let cursor = padding % selected.length;
    const sources = [];
    for (const entry of selected) {
      const bytes = fs.readFileSync(path.join(corpusRoot, entry.path));
      if (hash(bytes) !== entry.sha256) throw Error("Source WAV hash mismatch");
      const data = pcm(bytes);
      if (data.length !== entry.samples * 2)
        throw Error("Source sample count mismatch");
      cursor += gap;
      data.copy(audio, cursor * 2);
      sources.push({
        id: entry.id,
        sha256: entry.sha256,
        start: cursor,
        end: cursor + entry.samples,
      });
      cursor += entry.samples;
    }
    if (cursor !== 60 * 16000 - 1600) throw Error("Derived duration mismatch");
    const id = `${language}-long-60s`;
    const bytes = wav(audio);
    fs.writeFileSync(path.join(directory, `${id}.wav`), bytes);
    entries.push({
      id,
      language,
      path: `${id}.wav`,
      samples: 60 * 16000,
      sha256: hash(bytes),
      pcm_sha256: hash(audio),
      decoded_pcm_sha256: decodedPcmHash(audio),
      sources,
      reference: selected.map((entry) => entry.reference).join(" "),
    });
  }
  return {
    schema: 1,
    corpus: "fono-fleurs-derived-long-v1",
    license: corpus.license,
    attribution: corpus.attribution,
    source_card: corpus.source_card,
    license_url: corpus.license_url,
    source_manifest_sha256: hash(
      fs.readFileSync(path.join(corpusRoot, "manifest.json")),
    ),
    recipe:
      "Consecutive whole CI-03 clips; silence distributed before/between clips; final 100ms silence; exactly 60s.",
    limitations:
      "Two derived multi-speaker smoke WAVs, not natural monologues or owner microphone acceptance.",
    entries,
  };
}
export function summarize(run, corpus) {
  if (
    !run.completed ||
    run.entries?.length !== corpus.entries.length ||
    !["CPU", "Vulkan"].includes(run.device)
  )
    throw Error("Incomplete paired replay");
  const entries = run.entries.map((entry, index) => {
    const expected = corpus.entries[index];
    if (
      entry.id !== expected.id ||
      entry.pcm_sha256 !== expected.decoded_pcm_sha256 ||
      entry.samples !== expected.samples ||
      entry.baseline.device !== run.device ||
      entry.capture.device !== run.device
    )
      throw Error("Paired replay identity mismatch");
    for (const variant of [entry.baseline, entry.capture]) {
      if (
        typeof variant.text !== "string" ||
        !Number.isFinite(variant.stop_ready_seconds) ||
        variant.stop_ready_seconds < 0
      )
        throw Error("Invalid recognizer result");
    }
    let cursor = 0;
    for (const range of [...entry.capture.prefix, ...entry.capture.tail]) {
      if (
        range.start !== cursor ||
        !Number.isInteger(range.end) ||
        range.end <= cursor ||
        range.end > entry.samples
      )
        throw Error("Lost or duplicated PCM partition");
      cursor = range.end;
    }
    if (
      cursor !== entry.samples ||
      entry.capture.feed_seconds < 60 ||
      entry.capture.feed_seconds > 65 ||
      entry.service_scheduler.entered_before_interactive_finish !== false
    )
      throw Error("Invalid real-time replay/lifecycle");
    const baseline = score(expected.reference, entry.baseline.text);
    const capture = score(expected.reference, entry.capture.text);
    return {
      ...entry,
      baseline: { ...entry.baseline, errors: baseline },
      capture: { ...entry.capture, errors: capture },
      wer_not_worse: capture.word_errors <= baseline.word_errors,
      stop_latency_improved:
        entry.capture.stop_ready_seconds < entry.baseline.stop_ready_seconds,
      tail_reused:
        !entry.capture.full_batch_fallback && entry.capture.processed_until > 0,
    };
  });
  if (
    !run.cancel ||
    run.cancel.recognizer_active_at_cancel !== true ||
    run.cancel.active_inferences_after_join !== 0 ||
    run.cancel.scheduler_busy_after_join !== false ||
    run.cancel.activity_busy_after_cleanup !== false
  )
    throw Error("Cancellation left active work");
  return {
    entries,
    acceptance: {
      wer_not_worse_each_language: entries.every(
        (entry) => entry.wer_not_worse,
      ),
      prefix_reused_each_language: entries.every((entry) => entry.tail_reused),
      latency_improved_each_language: entries.every(
        (entry) => entry.stop_latency_improved,
      ),
      cancel_drained: true,
      default_enabled: false,
    },
  };
}
async function main(args) {
  if (args.length !== 4 || !["cpu", "vulkan"].includes(args[2]))
    throw Error(
      "usage: node scripts/long-dictation-quality.mjs NATIVE_TEST_EXE TINY_MODEL cpu|vulkan OUTPUT_DIR",
    );
  const [runner, model, backend, output] = [
    path.resolve(args[0]),
    path.resolve(args[1]),
    args[2],
    path.resolve(args[3]),
  ];
  if (hash(fs.readFileSync(model)) !== modelSpec.sha256)
    throw Error("Pinned Tiny model hash mismatch");
  fs.mkdirSync(output, { recursive: true });
  const source = read(path.join(corpusRoot, "manifest.json"));
  validateCorpus(source, corpusRoot);
  const corpus = derive(source, output);
  const corpusPath = path.join(output, "corpus.json");
  fs.writeFileSync(corpusPath, JSON.stringify(corpus, null, 2) + "\n");
  const lineage = qualityLineage();
  const files = new Map(
    lineage.source_inputs.map((entry) => [entry.path, entry.sha256]),
  );
  function visit(relative) {
    const absolute = path.join(root, relative);
    if (fs.statSync(absolute).isDirectory()) {
      for (const name of fs.readdirSync(absolute).sort())
        visit(`${relative}/${name}`);
    } else if (/\.(rs|mjs)$/.test(relative))
      files.set(relative, hash(fs.readFileSync(absolute)));
  }
  visit("src-tauri/src");
  visit("scripts/long-dictation-quality.mjs");
  lineage.source_inputs = [...files]
    .sort(([a], [b]) => a.localeCompare(b, "en"))
    .map(([path, sha256]) => ({ path, sha256 }));
  lineage.source_inputs_sha256 = hash(
    Buffer.from(JSON.stringify(lineage.source_inputs)),
  );
  const reportPath = path.join(output, "report.json");
  const rawPath = path.join(output, "native.json");
  const worker = [
    "src-tauri/resources/stt-workers",
    "src-tauri/stt-workers",
    "src-tauri/resources/stt-base",
  ]
    .map((directory) =>
      path.join(root, directory, "fono-stt-vulkan-worker.exe"),
    )
    .find(
      (candidate) =>
        fs.existsSync(candidate) && fs.statSync(candidate).isFile(),
    );
  const report = {
    schema: 1,
    lineage,
    created_at: new Date().toISOString(),
    backend,
    machine: {
      os: os.release(),
      arch: os.arch(),
      cpu: os.cpus()[0]?.model,
      logical_cpus: os.cpus().length,
    },
    model: modelSpec,
    model_sha256: modelSpec.sha256,
    runner_sha256: hash(fs.readFileSync(runner)),
    vad_sha256: hash(
      fs.readFileSync(
        path.join(root, "src-tauri/resources/vad/silero_vad.onnx"),
      ),
    ),
    native_runtime: RUNTIME_DLLS.map((filename) => ({
      filename,
      sha256: hash(fs.readFileSync(path.join(path.dirname(runner), filename))),
    })),
    worker_sha256: backend === "vulkan" ? hash(fs.readFileSync(worker)) : null,
    worker_relative_path:
      backend === "vulkan"
        ? path.relative(root, worker).replaceAll("\\", "/")
        : null,
    corpus,
    completed: false,
    quality_policy:
      "Paired ordinary batch baseline, same PCM/model/settings/backend; no WER margin.",
  };
  const save = () =>
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2) + "\n");
  save();
  const result = spawnSync(
    runner,
    [
      "application::dictation::segment_completion::quality::long_dictation_public_replay",
      "--exact",
      "--ignored",
      "--nocapture",
      "--test-threads=1",
    ],
    {
      cwd: root,
      windowsHide: true,
      encoding: "utf8",
      timeout: 245000,
      maxBuffer: 8 * 1024 * 1024,
      env: {
        ...process.env,
        FONO_LONG_RUNNER_PID: String(process.pid),
        FONO_LONG_MODEL: model,
        FONO_LONG_BACKEND: backend,
        FONO_LONG_CORPUS: corpusPath,
        FONO_LONG_OUTPUT: rawPath,
      },
    },
  );
  fs.writeFileSync(
    path.join(output, "native.log"),
    (result.stdout || "") + (result.stderr || ""),
  );
  if (fs.existsSync(rawPath)) report.native = read(rawPath);
  report.process = { status: result.status, error: result.error?.code || null };
  save();
  if (result.status !== 0) throw Error(`Native replay failed; see ${output}`);
  const summary = summarize(report.native, corpus);
  Object.assign(report, summary, { completed: true });
  save();
  console.log(
    JSON.stringify(
      {
        report: reportPath,
        acceptance: report.acceptance,
        entries: report.entries.map((entry) => ({
          id: entry.id,
          baseline_wer: entry.baseline.errors.wer,
          capture_wer: entry.capture.errors.wer,
          baseline_stop: entry.baseline.stop_ready_seconds,
          capture_stop: entry.capture.stop_ready_seconds,
          tail_seconds: entry.capture.remaining_seconds,
        })),
      },
      null,
      2,
    ),
  );
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
