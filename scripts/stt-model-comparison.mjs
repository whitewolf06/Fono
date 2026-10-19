// FONO-119: one bounded same-machine comparison of the production CPU recognizers.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { aggregate, hash, validateCorpus } from "./stt-quality.mjs";
import { score } from "./stage5-quality.mjs";
import { qualityLineage } from "./stt-quality-lineage.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const corpusRoot = path.join(root, "src-tauri/tests/fixtures/stt-quality");
const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
async function fileHash(file) {
  const digest = createHash("sha256");
  for await (const chunk of fs.createReadStream(file)) digest.update(chunk);
  return digest.digest("hex");
}
export function inputIdentity(run, corpus) {
  if (run.preprocessing?.length !== 20)
    throw Error("Missing complete preprocessing evidence");
  const identity = run.preprocessing.map((input, index) => {
    const expected = corpus.entries[index];
    if (
      input.id !== expected.id ||
      input.source_sha256 !== expected.sha256 ||
      input.samples !== expected.samples ||
      !/^[a-f0-9]{64}$/.test(input.pcm_sha256) ||
      typeof input.has_speech !== "boolean"
    )
      throw Error("PCM/corpus identity mismatch");
    let end = 0;
    for (const segment of input.segments) {
      if (
        segment.start !== end ||
        !Number.isInteger(segment.end) ||
        segment.end <= end ||
        segment.end - end > 25 * 16000
      )
        throw Error("Incomplete or unbounded PCM partition");
      end = segment.end;
    }
    if (end !== input.samples) throw Error("Segmentation dropped PCM");
    const { seconds, ...stable } = input;
    if (!Number.isFinite(seconds) || seconds < 0)
      throw Error("Invalid preprocessing timing");
    return stable;
  });
  return hash(Buffer.from(JSON.stringify(identity)));
}
export function summarize(run, corpus, languages) {
  if (
    !run.completed ||
    run.device !== "CPU" ||
    run.entries?.length !== 20 ||
    run.cold?.id !== "ru-01"
  )
    throw Error("Incomplete CPU comparison");
  for (const value of [run.model_load_seconds, run.cold.seconds])
    if (!Number.isFinite(value) || value < 0)
      throw Error("Invalid cold timing");
  const entries = run.entries.map((entry, index) => {
    const expected = corpus.entries[index];
    if (entry.id !== expected.id || entry.language !== expected.language)
      throw Error("Corpus order/language mismatch");
    if (!languages.includes(entry.language)) {
      if (entry.status !== "unsupported")
        throw Error("Unsupported language entered quality ranking");
      return null;
    }
    if (
      entry.status !== "completed" ||
      typeof entry.text !== "string" ||
      !Number.isFinite(entry.warm_seconds) ||
      entry.warm_seconds < 0 ||
      Math.abs(entry.audio_seconds - expected.samples / 16000) > 0.001
    )
      throw Error("Invalid completed utterance");
    return { ...entry, errors: score(expected.reference, entry.text) };
  });
  const summary = Object.fromEntries(
    ["ru", "en"].map((language) => {
      const subset = entries.filter((entry) => entry?.language === language);
      if (!languages.includes(language)) return [language, null];
      if (subset.length !== 10) throw Error("Language sample was dropped");
      const result = aggregate(subset);
      return [
        language,
        {
          ...result,
          utterances: subset.length,
          mean_warm_seconds: result.warm_seconds / subset.length,
        },
      ];
    }),
  );
  return {
    summary,
    entries: entries.filter(Boolean),
    input_identity: inputIdentity(run, corpus),
  };
}
export function modelFingerprint(files) {
  return [...files]
    .sort((a, b) => a.filename.localeCompare(b.filename, "en"))
    .map((file) => `${file.filename}:${file.bytes}:${file.sha256}`)
    .join("|");
}
async function main(args) {
  if (args.length !== 4)
    throw Error(
      "usage: node scripts/stt-model-comparison.mjs NATIVE_TEST_EXE WHISPER_DIR OFFLINE_DIR REPORT",
    );
  const [runner, whisper, offline, reportPath] = args.map((value) =>
    path.resolve(value),
  );
  const corpus = read(path.join(corpusRoot, "manifest.json"));
  validateCorpus(corpus, corpusRoot);
  const packages = read(
    path.join(root, "src-tauri/src/stt/offline_models.json"),
  );
  const quantized = read(
    path.join(root, "src-tauri/resources/whisper-quantized-models.json"),
  );
  const baseline = quantized.baselines.find(
    (model) => model.variant === "large_turbo",
  );
  const models = [
    ...packages.map((spec) => ({
      id: spec.id,
      path: path.join(offline, spec.directory),
      languages: spec.languages,
      artifacts: spec.files,
      revision: spec.revision,
    })),
    {
      id: { engine: "whisper", variant: baseline.variant },
      path: path.join(whisper, baseline.filename),
      languages: ["ru", "en"],
      revision: quantized.revision,
      artifacts: [
        {
          ...baseline,
          url: `https://huggingface.co/${quantized.repository}/resolve/${quantized.revision}/${baseline.filename}`,
        },
      ],
    },
  ];
  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  const report = {
    schema: 1,
    task: "FONO-119",
    started_at: new Date().toISOString(),
    lineage: qualityLineage(),
    collector_sha256: hash(fs.readFileSync(fileURLToPath(import.meta.url))),
    native_runner_sha256: await fileHash(runner),
    runner_source_sha256: hash(
      fs.readFileSync(path.join(root, "src-tauri/src/stt/comparison_tests.rs")),
    ),
    hardware: {
      cpu: os.cpus()[0]?.model,
      logical_cpus: os.cpus().length,
      os: os.release(),
      arch: os.arch(),
      memory_bytes: os.totalmem(),
      initial_free_memory_bytes: os.freemem(),
    },
    corpus: {
      name: corpus.corpus,
      sha256: hash(fs.readFileSync(path.join(corpusRoot, "manifest.json"))),
      utterances: 20,
      languages: { ru: 10, en: 10 },
      license: corpus.license,
      source: corpus.source_card,
    },
    policy: {
      backend: "CPU",
      whisper: "fast / 8 threads",
      sherpa: "greedy / 2 threads (production defaults)",
      cold: "same ru-01 before warm series",
      order: models.map((model) => model.id),
      maximum_model_seconds: 360,
      scope:
        "same production PCM and 25s segmentation; native decode timing excludes preprocessing/model load/LLM/insertion; one sequential run, no statistical speed ranking",
    },
    runs: [],
  };
  const save = () =>
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2) + "\n");
  save();
  for (const model of models) {
    const key =
      model.id.engine === "whisper"
        ? model.id.variant
        : `${model.id.engine}:${model.id.variant}`;
    console.log("CPU comparison", key);
    const evidence = {
      id: model.id,
      revision: model.revision,
      artifacts: model.artifacts,
      fingerprint: modelFingerprint(model.artifacts),
      languages: model.languages,
      status: "pending",
    };
    report.runs.push(evidence);
    save();
    try {
      for (const artifact of model.artifacts) {
        const file =
          model.id.engine === "whisper"
            ? model.path
            : path.join(model.path, artifact.filename);
        if (
          fs.statSync(file).size !== artifact.bytes ||
          (await fileHash(file)) !== artifact.sha256
        )
          throw Error(`Artifact mismatch: ${key}/${artifact.filename}`);
      }
      const nativeOutput = reportPath + `.${model.id.engine}.native.json`,
        log = fs.openSync(reportPath + `.${model.id.engine}.log`, "w");
      let execution;
      try {
        execution = spawnSync(
          runner,
          [
            "--exact",
            "stt::comparison_tests::public_corpus_model_comparison",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
          ],
          {
            env: {
              ...process.env,
              FONO_COMPARISON_SELECTION: JSON.stringify({
                id: model.id,
                local_path: model.path,
              }),
              FONO_COMPARISON_OUTPUT: nativeOutput,
            },
            stdio: ["ignore", log, log],
            timeout: 360000,
            windowsHide: true,
          },
        );
      } finally {
        fs.closeSync(log);
      }
      const raw = fs.existsSync(nativeOutput) ? read(nativeOutput) : null;
      if (execution.error || execution.status !== 0) {
        evidence.status = "incomplete";
        evidence.completed_utterances =
          raw?.entries.filter((entry) => entry.status === "completed").length ||
          0;
        evidence.reason =
          execution.error?.code || `native exit ${execution.status}`;
        save();
        continue;
      }
      if (
        raw.model.engine !== model.id.engine ||
        raw.model.variant !== model.id.variant ||
        raw.corpus_sha256 !== report.corpus.sha256
      )
        throw Error("Native model/corpus mismatch");
      const checked = summarize(raw, corpus, model.languages);
      Object.assign(evidence, { status: "completed", ...checked, raw });
      console.log(key, JSON.stringify(checked.summary));
      save();
    } catch (error) {
      evidence.status = "failed";
      evidence.reason = error.message;
      save();
    }
  }
  report.same_preprocessing =
    new Set(
      report.runs
        .filter((run) => run.status === "completed")
        .map((run) => run.input_identity),
    ).size === 1;
  report.completed =
    report.same_preprocessing &&
    report.runs.every((run) => run.status === "completed");
  report.completed_at = new Date().toISOString();
  save();
  if (!report.completed) process.exitCode = 1;
}
if (
  process.argv[1] &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
)
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
