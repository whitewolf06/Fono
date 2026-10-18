import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  aggregate,
  compare,
  hash,
  modelSpec,
  parseOptions,
  qualityExitCode,
  replayArguments,
  replayPolicy,
  resamplingAcceptance,
  validateCorpus,
} from "./stt-quality.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const directory = path.join(root, "src-tauri/tests/fixtures/stt-quality");
const manifest = JSON.parse(
  fs.readFileSync(path.join(directory, "manifest.json")),
);
test("fixed public corpus retains all audio hashes and both languages", () => {
  validateCorpus(manifest, directory);
  assert.equal(
    manifest.entries.filter((entry) => entry.condition.includes("noise"))
      .length,
    2,
  );
  assert.equal(
    manifest.entries.filter((entry) => entry.condition.includes("quiet"))
      .length,
    2,
  );
});
test("changed audio cannot silently establish a new quality baseline", () => {
  const changed = structuredClone(manifest);
  changed.entries[0].sha256 = "0".repeat(64);
  assert.throws(() => validateCorpus(changed, directory), /hash mismatch/);
});
test("WER and CER are micro-averaged from real counts, not averaged clip ratios", () => {
  const result = aggregate([
    {
      errors: {
        word_errors: 1,
        reference_words: 1,
        character_errors: 2,
        reference_characters: 2,
      },
      warm_seconds: 1,
      audio_seconds: 5,
    },
    {
      errors: {
        word_errors: 0,
        reference_words: 9,
        character_errors: 0,
        reference_characters: 18,
      },
      warm_seconds: 2,
      audio_seconds: 5,
    },
  ]);
  assert.equal(result.wer, 0.1);
  assert.equal(result.cer, 0.1);
  assert.equal(result.rtf, 0.3);
  assert.throws(() => aggregate([]), /denominator/);
});
test("the one percentage point regression gate fails beyond its threshold", () => {
  assert.equal(
    compare({ wer: 0.2, cer: 0.1 }, { wer: 0.21, cer: 0.1 }).no_wer_regression,
    false,
  );
  assert.equal(
    compare({ wer: 0.2, cer: 0.1 }, { wer: 0.21, cer: 0.1 }).passed,
    true,
  );
  assert.equal(
    compare({ wer: 0.2, cer: 0.1 }, { wer: 0.211, cer: 0.1 }).passed,
    false,
  );
});

const summary = (wordErrors) => ({ wer: wordErrors / 450, cer: 207 / 2846 });
function reportFor(legacy, candidate, historical, required) {
  return {
    comparison: compare(legacy, candidate),
    historical_comparison: historical ? compare(historical, candidate) : null,
    acceptance: resamplingAcceptance(legacy, candidate, historical, required),
  };
}
test("strict PERF-04 fails a single added word while the CI-03 allowance is retained", () => {
  const legacy = summary(85);
  const candidate = summary(86);
  const optional = reportFor(legacy, candidate, legacy, false);
  assert.equal(optional.comparison.passed, true);
  assert.equal(optional.comparison.maximum_wer_regression, 0.01);
  assert.equal(optional.acceptance.passed, false);
  assert.equal(qualityExitCode(optional), 0);
  const strict = reportFor(legacy, candidate, legacy, true);
  assert.equal(strict.acceptance.maximum_wer_regression, 0);
  assert.equal(strict.acceptance.same_run.passed, false);
  assert.equal(strict.acceptance.historical.passed, false);
  assert.equal(qualityExitCode(strict), 1);
  for (const errors of [85, 84])
    assert.equal(
      qualityExitCode(reportFor(legacy, summary(errors), legacy, true)),
      0,
    );
});
test("strict PERF-04 requires both same-run and frozen historical comparisons", () => {
  // A regressed same-run legacy cannot make a candidate pass the frozen gate.
  const historicalFailure = reportFor(
    summary(87),
    summary(86),
    summary(85),
    true,
  );
  assert.equal(historicalFailure.acceptance.same_run.passed, true);
  assert.equal(historicalFailure.acceptance.historical.passed, false);
  assert.equal(qualityExitCode(historicalFailure), 1);
  // A better same-run legacy cannot be ignored in favour of the historical WER.
  const sameRunFailure = reportFor(summary(84), summary(85), summary(85), true);
  assert.equal(sameRunFailure.acceptance.same_run.passed, false);
  assert.equal(sameRunFailure.acceptance.historical.passed, true);
  assert.equal(qualityExitCode(sameRunFailure), 1);
  const missing = reportFor(summary(85), summary(85), null, true);
  assert.equal(missing.acceptance.historical, null);
  assert.equal(missing.acceptance.passed, false);
  assert.equal(qualityExitCode(missing), 1);
  assert.equal(
    qualityExitCode(reportFor(summary(85), summary(90), summary(85), false)),
    1,
  );
});
test("acceptance is scoped to one explicit reconstruction method, not overall PERF-04", () => {
  for (const policy of ["legacy_linear", "cardinal_sinc"]) {
    const acceptance = resamplingAcceptance(
      summary(85),
      summary(85),
      summary(85),
      true,
      policy,
    );
    assert.equal(acceptance.reconstruction_policy, policy);
    assert.match(acceptance.criterion, new RegExp(policy));
    assert.match(acceptance.scope, /overall PERF-04 acceptance is separate/);
    assert.equal(acceptance.passed, true);
    assert.equal(
      qualityExitCode({ comparison: { passed: true }, acceptance }),
      0,
    );
  }
  assert.throws(
    () =>
      resamplingAcceptance(
        summary(85),
        summary(85),
        summary(85),
        true,
        "native",
      ),
    /Unknown reconstruction/,
  );
});
test("quality comparisons reject nonfinite and negative metrics or tolerances", () => {
  for (const bad of [Number.NaN, Infinity, -Infinity, -0.01, "0", null]) {
    assert.throws(() => compare({ wer: bad, cer: 0 }, summary(85)), /Invalid/);
    assert.throws(() => compare(summary(85), { wer: bad, cer: 0 }), /Invalid/);
    assert.throws(() => compare({ wer: 0, cer: bad }, summary(85)), /Invalid/);
    assert.throws(() => compare(summary(85), { wer: 0, cer: bad }), /Invalid/);
    assert.throws(() => compare(summary(85), summary(85), bad), /Invalid/);
  }
});
test("strict quality CLI flag is explicit and malformed options fail closed", () => {
  assert.deepEqual(parseOptions([]), {});
  assert.deepEqual(
    parseOptions([
      "--require-no-wer-regression",
      "--report",
      "result.json",
      "--download-model",
      "--bandlimited-reconstruction",
    ]),
    {
      "--require-no-wer-regression": true,
      "--report": "result.json",
      "--download-model": true,
      "--bandlimited-reconstruction": true,
    },
  );
  for (const args of [
    ["--require-no-wer-regression", "false"],
    ["--require-no-wer-regression", "--require-no-wer-regression"],
    ["--bandlimited-reconstruction", "false"],
    ["--bandlimited-reconstruction", "--bandlimited-reconstruction"],
    ["--model"],
    ["--model", "--require-no-wer-regression"],
    ["--report", ""],
    ["--maximum-wer-regression", "NaN"],
    ["--maximum-wer-regression", "-1"],
    ["--unknown"],
  ])
    assert.throws(() => parseOptions(args), /quality option/);
});

test("bandlimited replay is explicit, forwarded to both converters and identified in evidence", () => {
  const linear = parseOptions([]);
  const bandlimited = parseOptions(["--bandlimited-reconstruction"]);
  for (const mode of ["legacy", "current"]) {
    assert.deepEqual(replayArguments("input.wav", "output.wav", mode, linear), [
      "input.wav",
      "output.wav",
      mode,
      "48000",
    ]);
    assert.deepEqual(
      replayArguments("input.wav", "output.wav", mode, bandlimited),
      [
        "input.wav",
        "output.wav",
        mode,
        "48000",
        "--bandlimited-reconstruction",
      ],
    );
  }
  assert.equal(replayPolicy(linear).reconstruction_policy, "legacy_linear");
  assert.equal(
    replayPolicy(bandlimited).reconstruction_policy,
    "cardinal_sinc",
  );
  assert.match(replayPolicy(linear).replay, /frozen linear interpolation/);
  assert.match(
    replayPolicy(bandlimited).replay,
    /source Nyquist 8 kHz, half-window 48 source samples/,
  );
  assert.match(
    replayPolicy(bandlimited).replay,
    /frozen legacy PCM hashes are retained/,
  );
  for (const policy of [linear, bandlimited])
    assert.match(replayPolicy(policy).replay, /not native wideband audio/);
});

export function validateStrictResamplingWorkflow(workflow) {
  assert.match(workflow, /fail-fast: false/);
  assert.match(workflow, /reconstruction: \[legacy_linear, cardinal_sinc\]/);
  assert.match(
    workflow,
    /name: quality \(\$\{\{ matrix\.reconstruction \}\}\)/,
  );
  assert.doesNotMatch(workflow, /continue-on-error/);
  assert.match(
    workflow,
    /if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}/,
  );
  const name = "Compare legacy and current resampling with pinned public model";
  const step = workflow.split(`- name: ${name}\n`)[1]?.split(/\n\s+- name:/)[0];
  assert.ok(step, "Missing resampling step");
  assert.match(
    step,
    /node scripts\/stt-quality\.mjs [^\n]*--require-no-wer-regression(?:\s|$)/,
  );
  assert.match(
    step,
    /if \('\$\{\{ matrix\.reconstruction \}\}' -eq 'cardinal_sinc'\) \{\s+\$reconstructionArguments \+= '--bandlimited-reconstruction'\s+\}/,
  );
  assert.match(step, /\$reconstructionArguments = @\(\)/);
  assert.match(
    step,
    /--report build\/stt-quality\/report\.json @reconstructionArguments\s+exit \$LASTEXITCODE/,
  );
  assert.doesNotMatch(
    step,
    /continue-on-error|\bif:|\|\||;\s*(?:exit|true)|\bexit\s+0/,
  );
  const artifact = workflow.split(
    "- name: Save numerical quality evidence\n",
  )[1];
  assert.ok(artifact, "Missing quality artifact step");
  assert.match(artifact, /if: always\(\)/);
  assert.match(
    artifact,
    /name: stt-quality-cpu-\$\{\{ matrix\.reconstruction \}\}/,
  );
  assert.match(artifact, /path: \|\s+build\/stt-quality\/report\.json/);
  const aggregateJob = workflow.split("  quality-result:\n")[1];
  assert.ok(aggregateJob, "Missing stable required quality check");
  assert.match(aggregateJob, /name: quality\n/);
  assert.match(aggregateJob, /needs: \[quality\]/);
  assert.match(aggregateJob, /if: \$\{\{ always\(\) \}\}/);
  assert.match(
    aggregateJob,
    /QUALITY_RESULT: \$\{\{ needs\.quality\.result \}\}/,
  );
  assert.match(aggregateJob, /run: test "\$QUALITY_RESULT" = success/);
}
test("public resampling workflow cannot silently skip or soften the PERF-04 gate", () => {
  const workflow = fs
    .readFileSync(path.join(root, ".github/workflows/stt-quality.yml"), "utf8")
    .replaceAll("\r\n", "\n");
  validateStrictResamplingWorkflow(workflow);
  const strict = "--require-no-wer-regression";
  for (const changed of [
    workflow.replace(strict, ""),
    workflow.replace(strict, "--allow-regression"),
    workflow.replace("--bandlimited-reconstruction", ""),
    workflow.replace("fail-fast: false", "fail-fast: true"),
    workflow.replace("[legacy_linear, cardinal_sinc]", "[cardinal_sinc]"),
    workflow.replace("[legacy_linear, cardinal_sinc]", "[legacy_linear]"),
    workflow.replace("-eq 'cardinal_sinc'", "-eq 'legacy_linear'"),
    workflow.replace("exit $LASTEXITCODE", "exit 0"),
    workflow.replace(
      "name: stt-quality-cpu-${{ matrix.reconstruction }}",
      "name: stt-quality-cpu",
    ),
    workflow.replace("if: always()", "if: success()"),
    workflow.replace("name: quality\n", "name: optional-quality\n"),
    workflow.replace("needs: [quality]", "needs: []"),
    workflow.replace("if: ${{ always() }}", "if: ${{ success() }}"),
    workflow.replace('run: test "$QUALITY_RESULT" = success', "run: true"),
    workflow.replace(
      'run: test "$QUALITY_RESULT" = success',
      'run: test "$QUALITY_RESULT" != failure',
    ),
    workflow.replace(
      "      - name: Compare legacy and current resampling with pinned public model\n",
      "      - name: Compare legacy and current resampling with pinned public model\n        if: false\n",
    ),
    workflow.replace(
      "      - name: Compare legacy and current resampling with pinned public model\n",
      "      - name: Compare legacy and current resampling with pinned public model\n        continue-on-error: true\n",
    ),
    workflow.replace(strict, strict + " || true"),
  ])
    assert.throws(() => validateStrictResamplingWorkflow(changed));
});

test("public quality CI runs forks without frontend secrets and retains failure evidence", () => {
  const workflow = fs.readFileSync(
    path.join(root, ".github/workflows/stt-quality.yml"),
    "utf8",
  );
  assert.match(workflow, /\bpull_request:/);
  assert.doesNotMatch(
    workflow,
    /pull_request_target|secrets\.|NODE_AUTH_TOKEN|npm ci|needs: frontend/,
  );
  assert.match(
    workflow,
    /cargo build --locked --release --no-default-features/,
  );
  assert.match(workflow, /if: always\(\)/);
  assert.match(workflow, /path: \|\s+build\/stt-quality\/report\.json/);
});

test("historical baseline fixes corpus references and the CPU model identity", () => {
  const baseline = JSON.parse(
    fs.readFileSync(path.join(directory, "baseline.json")),
  );
  assert.equal(
    baseline.corpus_manifest_sha256,
    hash(Buffer.from(JSON.stringify(manifest))),
  );
  assert.equal(baseline.model.sha256, modelSpec.sha256);
  assert.ok(baseline.summary.reference_words > 300);
  assert.ok(baseline.summary.reference_characters > 1000);
});
