// Export reviewed measurement facts; incomplete models never acquire numbers.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { hash } from "./stt-quality.mjs";
import { inputIdentity, summarize } from "./stt-model-comparison.mjs";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export function exportData(report) {
  if (
    report.schema !== 1 ||
    report.task !== "FONO-119" ||
    !report.same_preprocessing
  )
    throw Error("Comparison has no shared preprocessing evidence");
  const models = report.runs
    .filter((run) => run.status === "completed")
    .map((run) => ({
      id:
        run.id.engine === "whisper"
          ? run.id.variant
          : `${run.id.engine}:${run.id.variant}`,
      fingerprint: run.fingerprint,
      profile: run.raw.profile,
      loadSeconds: run.raw.model_load_seconds,
      coldSeconds: run.raw.cold.seconds,
      ru: run.summary.ru,
      en: run.summary.en,
    }));
  if (!models.length) throw Error("No completed model measurements");
  return {
    schema: 1,
    date: report.completed_at.slice(0, 10),
    cpu: report.hardware.cpu,
    corpus: report.corpus.name,
    utterancesPerLanguage: 10,
    complete: report.completed,
    evidence: "docs/archive/improvement-plan/evidence/fono119-comparison.json",
    models,
  };
}
function main(args) {
  if (args.length !== 1)
    throw Error("usage: node scripts/export-stt-comparison.mjs REPORT");
  const report = JSON.parse(fs.readFileSync(args[0]));
  const corpus = JSON.parse(
    fs.readFileSync(
      path.join(root, "src-tauri/tests/fixtures/stt-quality/manifest.json"),
    ),
  );
  for (const run of report.runs.filter((run) => run.status === "completed")) {
    Object.assign(run, summarize(run.raw, corpus, run.languages));
  }
  for (const run of report.runs.filter((run) => run.status === "incomplete")) {
    const partial = `${args[0]}.${run.id.engine}.native.json`;
    if (fs.existsSync(partial)) {
      const raw = JSON.parse(fs.readFileSync(partial));
      run.partial_input_identity = inputIdentity(raw, corpus);
      run.partial_raw = raw;
    }
  }
  report.export_verification = {
    collector_sha256: hash(
      fs.readFileSync(path.join(root, "scripts/stt-model-comparison.mjs")),
    ),
    exporter_sha256: hash(fs.readFileSync(fileURLToPath(import.meta.url))),
    recomputed_completed_models: true,
  };
  const bytes = JSON.stringify(report, null, 2) + "\n";
  const data = exportData(report);
  data.evidenceSha256 = hash(Buffer.from(bytes));
  fs.writeFileSync(
    path.join(
      root,
      "docs/archive/improvement-plan/evidence/fono119-comparison.json",
    ),
    bytes,
  );
  fs.writeFileSync(
    path.join(root, "src/v3/shared/domain/sttMeasurements.json"),
    JSON.stringify(data, null, 2) + "\n",
  );
}
if (
  process.argv[1] &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
)
  main(process.argv.slice(2));
