// CI-03: consolidate one explicitly identified CPU replay, never historical runs.
import { randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import {
  aggregate,
  compare,
  hash,
  modelSpec,
  validateCorpus,
} from "./stt-quality.mjs";
import { qualityLineage } from "./stt-quality-lineage.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const directory = path.join(root, "src-tauri/tests/fixtures/stt-quality");
const names = [
  "legacy",
  "current",
  "original_fast",
  "original_accurate",
  "neural_fast",
  "neural_accurate",
];
const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
function requireThat(value, message) {
  if (!value) throw new Error(message);
}
function finite(value, label, positive = false) {
  requireThat(
    typeof value === "number" &&
      Number.isFinite(value) &&
      (positive ? value > 0 : value >= 0),
    `Invalid measurement: ${label}`,
  );
}
function same(left, right, label) {
  requireThat(isDeepStrictEqual(left, right), `Identity mismatch: ${label}`);
}
export function percentile(values, p) {
  requireThat(values.length > 0, "Empty latency population");
  requireThat(
    typeof p === "number" && p > 0 && p <= 1,
    "Invalid latency quantile",
  );
  const sorted = [...values].sort((a, b) => a - b);
  for (const value of sorted) finite(value, "latency sample");
  return sorted[Math.max(0, Math.ceil(p * sorted.length) - 1)];
}
const normalized = (text) =>
  text
    .toLowerCase()
    .replace(/[^\p{Alphabetic}\p{Number}]+/gu, " ")
    .trim();
// Independent verification of the Rust scorer: lowercase, retain Unicode
// Alphabetic/Number characters, collapse separators, then edit distance over
// words and Unicode scalar values (never UTF-16 code units).
function editDistance(expected, actual) {
  let previous = Array.from({ length: actual.length + 1 }, (_, index) => index);
  let current = new Array(actual.length + 1);
  for (let row = 0; row < expected.length; row++) {
    current[0] = row + 1;
    for (let column = 0; column < actual.length; column++)
      current[column + 1] = Math.min(
        previous[column] + Number(expected[row] !== actual[column]),
        current[column] + 1,
        previous[column + 1] + 1,
      );
    [previous, current] = [current, previous];
  }
  return previous[actual.length];
}
export function score(reference, hypothesis) {
  const expected = normalized(reference);
  const actual = normalized(hypothesis);
  const words = expected ? expected.split(" ") : [];
  const actualWords = actual ? actual.split(" ") : [];
  const characters = [...expected];
  const word_errors = editDistance(words, actualWords);
  const character_errors = editDistance(characters, [...actual]);
  return {
    word_errors,
    reference_words: words.length,
    character_errors,
    reference_characters: characters.length,
    wer: words.length ? word_errors / words.length : null,
    cer: characters.length ? character_errors / characters.length : null,
  };
}
function validateLineage(lineage, label) {
  requireThat(
    lineage &&
      typeof lineage.run_id === "string" &&
      lineage.run_id.trim().length > 0 &&
      /^[a-f0-9]{40}$/.test(lineage.source_revision) &&
      /^[a-f0-9]{64}$/.test(lineage.source_inputs_sha256) &&
      /^\d+\.\d+\.\d+$/.test(lineage.app_version),
    `Invalid report lineage: ${label}`,
  );
}
function validateVariant(variant, manifest, name) {
  requireThat(variant?.backend === "cpu", `Missing CPU variant: ${name}`);
  requireThat(
    variant.entries?.length === manifest.entries.length,
    `Coverage mismatch: ${name}`,
  );
  requireThat(
    Number.isInteger(variant.threads) &&
      variant.threads >= 1 &&
      variant.threads <= 8,
    `Invalid threads: ${name}`,
  );
  same(
    variant.decoding?.strategy,
    name.endsWith("accurate") ? "accurate" : "fast",
    `${name} decoding`,
  );
  requireThat(
    variant.decoding.threads === null ||
      variant.decoding.threads === variant.threads,
    `Invalid decoding threads: ${name}`,
  );
  finite(variant.model_load_seconds, `${name} model load`, true);
  requireThat(
    variant.cold &&
      manifest.entries.some((entry) => entry.id === variant.cold.id),
    `Missing cold inference: ${name}`,
  );
  finite(variant.cold.seconds, `${name} cold inference`, true);
  same(
    variant.cold.id,
    variant.entries.find((entry) => entry.no_speech !== true)?.id,
    `${name} first cold clip`,
  );
  const seen = new Set();
  for (let index = 0; index < manifest.entries.length; index++) {
    const source = manifest.entries[index];
    const entry = variant.entries[index];
    requireThat(
      entry && entry.id === source.id && !seen.has(entry.id),
      `Coverage/order mismatch: ${name}`,
    );
    seen.add(entry.id);
    same(entry.language, source.language, `${name}/${source.id} language`);
    requireThat(
      typeof entry.text === "string",
      `Missing hypothesis: ${name}/${source.id}`,
    );
    same(
      entry.errors,
      score(source.reference, entry.text),
      `${name}/${source.id} hypothesis error counts`,
    );
    finite(entry.audio_seconds, `${name}/${source.id} audio`, true);
    finite(entry.warm_seconds, `${name}/${source.id} warm`);
    finite(entry.rtf, `${name}/${source.id} RTF`);
    requireThat(
      Math.abs(entry.rtf - entry.warm_seconds / entry.audio_seconds) <= 1e-9,
      `Inconsistent RTF: ${name}/${source.id}`,
    );
    requireThat(
      Math.abs(entry.audio_seconds - source.samples / source.sample_rate) <=
        1 / 16000 + 1e-9,
      `Audio duration mismatch: ${name}/${source.id}`,
    );
    for (const metric of [
      "word_errors",
      "reference_words",
      "character_errors",
      "reference_characters",
    ])
      requireThat(
        Number.isSafeInteger(entry.errors?.[metric]) &&
          entry.errors[metric] >= 0,
        `Invalid error counts: ${name}/${source.id}/${metric}`,
      );
    const reference = normalized(source.reference);
    same(
      entry.errors.reference_words,
      reference.split(" ").length,
      `${name}/${source.id} reference words`,
    );
    same(
      entry.errors.reference_characters,
      [...reference].length,
      `${name}/${source.id} reference characters`,
    );
    finite(entry.errors.wer, `${name}/${source.id} WER`);
    finite(entry.errors.cer, `${name}/${source.id} CER`);
    requireThat(
      Math.abs(
        entry.errors.wer -
          entry.errors.word_errors / entry.errors.reference_words,
      ) <= 1e-12 &&
        Math.abs(
          entry.errors.cer -
            entry.errors.character_errors / entry.errors.reference_characters,
        ) <= 1e-12,
      `Inconsistent quality ratios: ${name}/${source.id}`,
    );
    if (entry.no_speech === true) {
      requireThat(
        entry.text === "" &&
          entry.warm_seconds === 0 &&
          entry.errors.word_errors === entry.errors.reference_words &&
          entry.errors.character_errors === entry.errors.reference_characters,
        `Speech rejection hid errors: ${name}/${source.id}`,
      );
    } else
      finite(entry.warm_seconds, `${name}/${source.id} warm inference`, true);
  }
  return variant;
}
function describe(variant, manifest) {
  const groups = {};
  for (const [label, predicate] of [
    ["ru", (entry) => entry.language === "ru"],
    ["en", (entry) => entry.language === "en"],
    ["quiet", (entry) => entry.condition.includes("quiet")],
    ["noise", (entry) => entry.condition.includes("noise")],
    [
      "short_under_10_seconds",
      (entry) => entry.samples / entry.sample_rate < 10,
    ],
    [
      "long_at_least_10_seconds",
      (entry) => entry.samples / entry.sample_rate >= 10,
    ],
  ]) {
    const ids = new Set(
      manifest.entries.filter(predicate).map((entry) => entry.id),
    );
    requireThat(ids.size > 0, `Missing corpus stratum: ${label}`);
    groups[label] = {
      utterances: ids.size,
      ...aggregate(variant.entries.filter((entry) => ids.has(entry.id))),
    };
  }
  const measured = variant.entries.filter((entry) => entry.no_speech !== true);
  requireThat(measured.length > 0, "No inference latency was measured");
  return {
    backend: variant.backend,
    threads: variant.threads,
    decoding: variant.decoding,
    summary: aggregate(variant.entries),
    per_clip: variant.entries.map((entry, index) => ({
      id: entry.id,
      language: entry.language,
      condition: manifest.entries[index].condition,
      reference: manifest.entries[index].reference,
      hypothesis: entry.text,
      errors: entry.errors,
      audio_seconds: entry.audio_seconds,
      warm_seconds: entry.warm_seconds,
      rtf: entry.rtf,
      no_speech: entry.no_speech === true,
    })),
    strata: groups,
    latency: {
      method:
        "fresh_model_context_then_first_clip_repeated;nearest_rank_quantiles_across_distinct_clips_v1",
      model_load_seconds: variant.model_load_seconds,
      first_inference: { id: variant.cold.id, seconds: variant.cold.seconds },
      first_ready_seconds: variant.model_load_seconds + variant.cold.seconds,
      warm_utterances: measured.length,
      speech_gate_rejections: variant.entries.length - measured.length,
      warm_seconds: Object.fromEntries(
        [0.5, 0.95, 0.99].map((p) => [
          `p${p * 100}`,
          percentile(
            measured.map((entry) => entry.warm_seconds),
            p,
          ),
        ]),
      ),
      warm_rtf: Object.fromEntries(
        [0.5, 0.95, 0.99].map((p) => [
          `p${p * 100}`,
          percentile(
            measured.map((entry) => entry.rtf),
            p,
          ),
        ]),
      ),
      per_clip: variant.entries.map(({ id, warm_seconds, no_speech }) => ({
        id,
        warm_seconds,
        no_speech: no_speech === true,
      })),
    },
  };
}
export function compareLatency(
  baseline,
  candidate,
  maximumRatio,
  slackSeconds,
) {
  finite(maximumRatio, "maximum latency ratio", true);
  finite(slackSeconds, "latency slack");
  requireThat(maximumRatio >= 1, "Maximum latency ratio must be at least 1");
  requireThat(
    baseline?.schema === 1 && baseline.kind === "fono-ci03-consolidated",
    "Invalid latency baseline schema",
  );
  validateLineage(baseline.lineage, "latency baseline");
  validateLineage(candidate.lineage, "latency candidate");
  requireThat(
    baseline.lineage.run_id !== candidate.lineage.run_id,
    "Latency baseline requires an independent run_id",
  );
  for (const key of ["corpus_manifest_sha256", "model", "hardware", "profile"])
    same(baseline[key], candidate[key], `latency ${key}`);
  const variants = {};
  for (const name of names) {
    const before = baseline.variants?.[name];
    const after = candidate.variants[name];
    requireThat(
      before &&
        before.latency?.per_clip?.length === after.latency.per_clip.length,
      `Missing latency baseline variant: ${name}`,
    );
    for (const key of ["backend", "threads", "decoding", "input_hashes"])
      same(before[key], after[key], `latency ${name}/${key}`);
    same(before.latency.method, after.latency.method, `latency ${name}/method`);
    same(
      before.latency.first_inference?.id,
      after.latency.first_inference.id,
      `latency ${name}/first clip`,
    );
    const samples = [
      {
        metric: "model_load_seconds",
        baseline: before.latency.model_load_seconds,
        candidate: after.latency.model_load_seconds,
      },
      {
        metric: "first_inference_seconds",
        baseline: before.latency.first_inference?.seconds,
        candidate: after.latency.first_inference.seconds,
      },
      ...after.latency.per_clip.map((clip, index) => {
        const previous = before.latency.per_clip[index];
        same(previous.id, clip.id, `latency ${name}/clip order`);
        same(
          previous.no_speech,
          clip.no_speech,
          `latency ${name}/${clip.id}/speech admission`,
        );
        return {
          metric: clip.id,
          baseline: previous.warm_seconds,
          candidate: clip.warm_seconds,
        };
      }),
    ];
    variants[name] = samples.map((sample) => {
      finite(sample.baseline, `latency baseline ${name}/${sample.metric}`);
      finite(sample.candidate, `latency candidate ${name}/${sample.metric}`);
      const limit_seconds = sample.baseline * maximumRatio + slackSeconds;
      return {
        ...sample,
        limit_seconds,
        passed: sample.candidate <= limit_seconds + 1e-12,
      };
    });
  }
  return {
    state: "evaluated_explicit_same_hardware_baseline",
    maximum_ratio: maximumRatio,
    absolute_slack_seconds: slackSeconds,
    variants,
    passed: Object.values(variants).every((samples) =>
      samples.every((sample) => sample.passed),
    ),
  };
}
export function consolidate(
  resampling,
  vad,
  manifest,
  frozenBaseline,
  options = {},
) {
  const manifestHash = hash(Buffer.from(JSON.stringify(manifest)));
  requireThat(
    manifest.entries.length >= 15 && manifest.entries.length <= 30,
    "Corpus size is outside CI-03 contract",
  );
  for (const [label, report] of [
    ["resampling", resampling],
    ["vad", vad],
  ]) {
    requireThat(
      report.schema === 1 && report.lineage,
      `Missing report lineage: ${label}`,
    );
    validateLineage(report.lineage, label);
    same(report.corpus_manifest_sha256, manifestHash, `${label} corpus`);
    same(report.model.sha256, modelSpec.sha256, `${label} model`);
    requireThat(
      typeof report.hardware?.cpu === "string" &&
        report.hardware.cpu.length > 0 &&
        Number.isInteger(report.hardware.logical_cpus) &&
        report.hardware.logical_cpus > 0,
      `Missing hardware identity: ${label}`,
    );
    requireThat(
      typeof report.profile === "string" && report.profile.length > 0,
      `Missing build profile: ${label}`,
    );
  }
  same(resampling.lineage, vad.lineage, "one run/source/version lineage");
  same(resampling.hardware, vad.hardware, "hardware");
  same(resampling.profile, vad.profile, "build profile");
  for (const value of [
    resampling.replay_binary_sha256,
    vad.binaries.trimmer_sha256,
    vad.vad_model_sha256,
  ])
    requireThat(/^[a-f0-9]{64}$/.test(value), "Invalid preprocessing identity");
  requireThat(
    vad.boundaries?.length === manifest.entries.length,
    "Missing VAD boundary coverage",
  );
  for (let index = 0; index < manifest.entries.length; index++) {
    const boundary = vad.boundaries[index];
    const entry = manifest.entries[index];
    same(boundary.id, entry.id, "VAD boundary order");
    same(boundary.input_samples, entry.samples, "VAD original samples");
    requireThat(
      typeof boundary.has_speech === "boolean" &&
        /^[a-f0-9]{64}$/.test(boundary.output_sha256),
      "Invalid VAD boundary identity",
    );
    for (const strategy of ["fast", "accurate"])
      same(
        vad.variants?.[`neural_${strategy}`]?.entries?.[index]?.no_speech ===
          true,
        !boundary.has_speech,
        "VAD admission result",
      );
  }
  same(
    resampling.runner_binary_sha256,
    vad.binaries.runner_sha256,
    "CPU runner binary",
  );
  requireThat(
    /^[a-f0-9]{64}$/.test(resampling.runner_binary_sha256),
    "Invalid CPU runner identity",
  );
  same(frozenBaseline.model.sha256, modelSpec.sha256, "frozen model");
  same(frozenBaseline.corpus_manifest_sha256, manifestHash, "frozen corpus");
  same(
    frozenBaseline.replay_hashes,
    resampling.alternatives?.legacy?.replay_hashes,
    "frozen replay",
  );
  const raw = { ...resampling.alternatives, ...vad.variants };
  const variants = Object.fromEntries(
    names.map((name) => [
      name,
      describe(validateVariant(raw[name], manifest, name), manifest),
    ]),
  );
  for (const name of names) {
    const hashes =
      name === "legacy" || name === "current"
        ? resampling.alternatives[name].replay_hashes
        : name.startsWith("neural")
          ? vad.boundaries.map((entry) => ({
              id: entry.id,
              sha256: entry.output_sha256,
            }))
          : manifest.entries.map(({ id, sha256 }) => ({ id, sha256 }));
    requireThat(
      hashes?.length === manifest.entries.length,
      `Missing input hashes: ${name}`,
    );
    for (let index = 0; index < manifest.entries.length; index++)
      requireThat(
        hashes[index].id === manifest.entries[index].id &&
          /^[a-f0-9]{64}$/.test(hashes[index].sha256),
        `Invalid input hashes: ${name}`,
      );
    variants[name].input_hashes = hashes;
  }
  for (const name of names)
    same(
      variants[name].threads,
      variants.legacy.threads,
      `${name} actual threads`,
    );
  const gates = {
    ci03_same_run_wer_plus_one_percentage_point: compare(
      variants.legacy.summary,
      variants.current.summary,
    ),
    ci03_historical_wer_plus_one_percentage_point: compare(
      frozenBaseline.summary,
      variants.current.summary,
    ),
    arc09_production_fast_no_wer_regression: compare(
      variants.original_fast.summary,
      variants.neural_fast.summary,
      0,
    ),
  };
  const report = {
    schema: 1,
    kind: "fono-ci03-consolidated",
    generated_at: new Date().toISOString(),
    lineage: resampling.lineage,
    corpus: manifest.corpus,
    corpus_manifest_sha256: manifestHash,
    license: manifest.license,
    attribution: manifest.attribution,
    source_card: manifest.source_card,
    license_url: manifest.license_url,
    model: {
      basename: modelSpec.basename,
      sha256: modelSpec.sha256,
      revision: modelSpec.revision,
    },
    vad_model_sha256: vad.vad_model_sha256,
    hardware: resampling.hardware,
    profile: resampling.profile,
    binaries: {
      runner_sha256: resampling.runner_binary_sha256,
      resampler_sha256: resampling.replay_binary_sha256,
      trimmer_sha256: vad.binaries.trimmer_sha256,
    },
    variants,
    quality_gates: gates,
    open_acceptance: {
      perf04_no_wer_regression: {
        ...compare(variants.legacy.summary, variants.current.summary, 0),
        task: "FONO-79",
        ci03_exit_gate: false,
        state:
          variants.current.summary.wer <= variants.legacy.summary.wer + 1e-12
            ? "numerical_criterion_met_on_derived_replay_only"
            : "failed_open",
      },
      accurate_vs_fast: compare(
        variants.original_fast.summary,
        variants.original_accurate.summary,
        0,
      ),
    },
    latency_acceptance: {
      state: "observational_no_independent_same_hardware_baseline",
      passed: null,
    },
    limitations: [
      "Twenty public 16 kHz clips: a smoke regression corpus, not native 48 kHz or owner microphone acceptance.",
      "Quiet/noise are deterministic transformations; long means >=10 seconds, including repeated real utterances, not independent 60-second speech.",
      "Cold is a fresh model/context, not a flushed OS cache. Quantiles span distinct clips and are not repeated same-clip jitter estimates.",
      "CPU-only inference excludes microphone, Tauri, Windows insertion and Stop-to-ready application latency.",
      "Source hashes identify the observed checkout; binary hashes do not attest its compiler provenance.",
    ],
  };
  if (options.latencyBaseline)
    report.latency_acceptance = compareLatency(
      options.latencyBaseline,
      report,
      options.maximumRatio,
      options.slackSeconds,
    );
  report.passed =
    Object.values(gates).every((gate) => gate.passed) &&
    report.latency_acceptance.passed !== false;
  return report;
}

async function main() {
  const flags = new Set(["--run", "--download-model"]);
  const values = new Set([
    "--runner",
    "--trimmer",
    "--resampler",
    "--model",
    "--profile",
    "--resampling-report",
    "--vad-report",
    "--report",
    "--latency-baseline",
    "--maximum-latency-ratio",
    "--latency-slack-seconds",
  ]);
  const options = {};
  for (let index = 2; index < process.argv.length; index++) {
    const key = process.argv[index];
    requireThat(
      !(key in options) && (flags.has(key) || values.has(key)),
      `Unknown or repeated option: ${key}`,
    );
    if (flags.has(key)) options[key] = true;
    else {
      requireThat(
        process.argv[index + 1] && !process.argv[index + 1].startsWith("--"),
        `Missing value: ${key}`,
      );
      options[key] = process.argv[++index];
    }
  }
  requireThat(
    options["--latency-baseline"]
      ? options["--maximum-latency-ratio"] && options["--latency-slack-seconds"]
      : !options["--maximum-latency-ratio"] &&
          !options["--latency-slack-seconds"],
    "Latency baseline, maximum ratio and slack must be supplied together explicitly",
  );
  const resamplingPath = path.resolve(
    options["--resampling-report"] ?? "build/stt-quality/report.json",
  );
  const vadPath = path.resolve(
    options["--vad-report"] ?? "build/stt-quality/stage4-report.json",
  );
  const childRuns = [];
  if (options["--run"]) {
    const env = {
      ...process.env,
      STT_QUALITY_RUN_ID: process.env.STT_QUALITY_RUN_ID || randomUUID(),
    };
    const common = ["--runner", "--model", "--profile"]
      .filter((key) => options[key])
      .flatMap((key) => [key, options[key]]);
    for (const [script, output, extra] of [
      [
        "stt-quality.mjs",
        resamplingPath,
        [
          ...(options["--resampler"]
            ? ["--resampler", options["--resampler"]]
            : []),
          ...(options["--download-model"] ? ["--download-model"] : []),
        ],
      ],
      [
        "stage4-quality.mjs",
        vadPath,
        options["--trimmer"] ? ["--trimmer", options["--trimmer"]] : [],
      ],
    ]) {
      fs.rmSync(output, { force: true });
      const child = spawnSync(
        process.execPath,
        [
          path.join(root, "scripts", script),
          ...common,
          ...extra,
          "--report",
          output,
        ],
        {
          cwd: root,
          env,
          stdio: "inherit",
          windowsHide: true,
          timeout: 1_200_000,
        },
      );
      if (child.error) throw child.error;
      childRuns.push({ script, exit_code: child.status });
      requireThat(
        fs.existsSync(output),
        `Replay failed to produce fresh evidence: ${script}`,
      );
    }
  }
  const manifest = read(path.join(directory, "manifest.json"));
  validateCorpus(manifest, directory);
  const report = consolidate(
    read(resamplingPath),
    read(vadPath),
    manifest,
    read(path.join(directory, "baseline.json")),
    {
      latencyBaseline: options["--latency-baseline"]
        ? read(options["--latency-baseline"])
        : undefined,
      maximumRatio: options["--maximum-latency-ratio"]
        ? Number(options["--maximum-latency-ratio"])
        : undefined,
      slackSeconds: options["--latency-slack-seconds"]
        ? Number(options["--latency-slack-seconds"])
        : undefined,
    },
  );
  const current = qualityLineage();
  for (const key of ["source_inputs_sha256", "source_revision", "app_version"])
    same(current[key], report.lineage[key], `current source snapshot ${key}`);
  report.inputs = [resamplingPath, vadPath].map((file) => ({
    path: path.relative(root, file).replaceAll("\\", "/"),
    sha256: hash(fs.readFileSync(file)),
  }));
  report.child_runs = childRuns;
  if (childRuns.some((child) => child.exit_code !== 0)) report.passed = false;
  const output = path.resolve(
    options["--report"] ?? "build/stt-quality/stage5-report.json",
  );
  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.writeFileSync(output, JSON.stringify(report, null, 2) + "\n");
  console.log(
    JSON.stringify({
      report: path.relative(root, output),
      passed: report.passed,
      perf04: report.open_acceptance.perf04_no_wer_regression.state,
      latency: report.latency_acceptance.state,
    }),
  );
  if (!report.passed) process.exitCode = 1;
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
)
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
