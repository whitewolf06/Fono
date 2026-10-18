import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  digest,
  limits,
  parseArgs,
  prepare,
  validateReport,
} from "./lifecycle-soak.mjs";
import { modelSpec } from "./stt-quality.mjs";

test("soak limits reject zero cycles, huge counts, invalid numerics and unbounded idle windows", () => {
  assert.deepEqual(limits({}), {
    cycles: 1000,
    faults: 100,
    active: 100,
    idle: 0,
    timeout: 7200,
  });
  for (const [key, value] of [
    ["--cycles", "0"],
    ["--cycles", "-1"],
    ["--cycles", "1.5"],
    ["--cycles", "100001"],
    ["--cycles", "1e3"],
    ["--faults", "0"],
    ["--faults", "10001"],
    ["--idle-seconds", "7201"],
    ["--timeout-seconds", "NaN"],
  ])
    assert.throws(() => limits({ [key]: value }));
  assert.throws(() =>
    limits({ "--cycles": "1", "--active-cancel-every": "2" }),
  );
  assert.throws(() =>
    limits({ "--idle-seconds": "3600", "--timeout-seconds": "3600" }),
  );
});
test("argument parsing rejects duplicates, switches without values and malformed options", () => {
  assert.deepEqual(parseArgs(["--cycles", "1000", "--faults", "100"]), {
    "--cycles": "1000",
    "--faults": "100",
  });
  for (const args of [
    ["--cycles"],
    ["cycles", "3"],
    ["--cycles", "3", "--cycles", "4"],
    ["--cycles", "--faults"],
  ])
    assert.throws(() => parseArgs(args));
});
test("input validation fails before any process launch for wrong platform, paths or binary hash", () => {
  assert.throws(() => prepare({}, "linux"), /Windows/);
  assert.throws(() => prepare({}, "win32"), /absolute input path/);
  assert.throws(() => prepare({ "--unknown": "x" }, "win32"), /Unknown/);
  const directory = fs.mkdtempSync(
    path.join(os.tmpdir(), "fono-soak-contract-"),
  );
  try {
    const runner = path.join(directory, "test.exe");
    fs.writeFileSync(runner, "driver");
    assert.throws(
      () =>
        prepare(
          { "--runner": runner, "--runner-sha256": "0".repeat(64) },
          "win32",
        ),
      /SHA256 differs/,
    );
    assert.throws(
      () =>
        prepare(
          {
            "--runner": runner,
            "--runner-sha256": digest(runner),
            "--worker": runner,
            "--worker-sha256": digest(runner),
          },
          "win32",
        ),
      /different binaries/,
    );
    const worker = path.join(directory, "cpu-worker.exe");
    const model = path.join(directory, "wrong-model.bin");
    fs.writeFileSync(worker, "different real-helper placeholder");
    fs.writeFileSync(model, "not the pinned public tiny model");
    assert.throws(
      () =>
        prepare(
          {
            "--runner": runner,
            "--runner-sha256": digest(runner),
            "--worker": worker,
            "--worker-sha256": digest(worker),
            "--model": model,
          },
          "win32",
        ),
      /SHA256 differs/,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
function fixture() {
  const config = {
    cycles: 1000,
    faults: 100,
    active: 100,
    idle: 0,
    workerSha256: "a".repeat(64),
    wavSha256: "b".repeat(64),
  };
  const report = {
    schema: 1,
    kind: "fono-production-subsystem-lifecycle-soak",
    status: "passed",
    limits: { start_cancel_cycles: 1000, worker_faults: 100 },
    completed: {
      start_cancel_cycles: 1000,
      fault_kills: 100,
      fault_reaps: 100,
      fault_restarts: 100,
      active_worker_cancellations: 10,
      pretransport_worker_cancellations: 990,
      all_owned_children_exited: true,
    },
    inputs: {
      worker_sha256: config.workerSha256,
      model_sha256: modelSpec.sha256,
      wav_sha256: config.wavSha256,
    },
    resource_samples: [{ phase: "warm_baseline" }, { phase: "final_shutdown" }],
    measured_idle_seconds: 0,
    resource_gates: Object.fromEntries(
      ["loaded_parent", "shutdown_parent"].map((phase) => [
        phase,
        { handles_pass: true, threads_pass: true, private_bytes_pass: true },
      ]),
    ),
  };
  const sample = {
    pid: 1,
    handles: 50,
    threads: 2,
    private_bytes: 1_000_000,
    cpu_seconds: 1,
  };
  report.resource_samples = [
    "cold_pre_worker",
    "warm_baseline",
    "final_loaded",
    "final_shutdown",
    "warm_no_worker_baseline",
  ].map((phase) => ({
    phase,
    parent: structuredClone(sample),
    worker:
      phase.includes("loaded") || phase === "warm_baseline"
        ? structuredClone(sample)
        : null,
  }));
  report.worker_faults = Array.from({ length: 100 }, (_, index) => ({
    index: index + 1,
    owned_pid: index + 2,
    alive_before_kill: true,
    confirmed_exit: true,
  }));
  report.latency_ms = Object.fromEntries(
    [
      "capture_start",
      "capture_stop",
      "scheduler_acquire_uncontended",
      "cancel_to_request_return",
    ].map((key) => [key, { count: 1000, p50: 1, p95: 2, p99: 3 }]),
  );
  return { config, report };
}
test("report gate requires actual cycle, kill, reap, restart and cancellation accounting", () => {
  const { config, report } = fixture();
  assert.equal(validateReport(report, config), report);
  for (const field of [
    "start_cancel_cycles",
    "fault_kills",
    "fault_reaps",
    "fault_restarts",
    "active_worker_cancellations",
    "pretransport_worker_cancellations",
  ]) {
    const invalid = structuredClone(report);
    invalid.completed[field]--;
    assert.throws(() => validateReport(invalid, config));
  }
});
test("report gate rejects fabricated success without measured resources, matching hashes or cleanup", () => {
  const { config, report } = fixture();
  for (const alter of [
    (r) => {
      r.status = "failed";
    },
    (r) => {
      r.resource_samples = [];
    },
    (r) => {
      r.inputs.worker_sha256 = "c".repeat(64);
    },
    (r) => {
      r.completed.all_owned_children_exited = false;
    },
    (r) => {
      r.resource_gates.shutdown_parent.handles_pass = false;
    },
    (r) => {
      r.resource_gates.loaded_parent.private_bytes_pass = false;
    },
    (r) => {
      delete r.resource_samples[0].parent.handles;
    },
    (r) => {
      r.worker_faults[0].confirmed_exit = false;
    },
    (r) => {
      r.latency_ms.capture_start.count = 999;
    },
    (r) => {
      r.resource_samples[0].parent.threads = 0;
    },
    (r) => {
      r.resource_samples[0].parent.handles = false;
    },
    (r) => {
      r.resource_samples[0].parent.private_bytes = -1;
    },
    (r) => {
      r.resource_samples[2].parent.handles++;
    },
    (r) => {
      r.resource_samples[3].parent.private_bytes += 11 * 1024 * 1024;
    },
    (r) => {
      r.resource_samples[1].worker = null;
    },
  ]) {
    const invalid = structuredClone(report);
    alter(invalid);
    assert.throws(() => validateReport(invalid, config));
  }
});
test("idle measurement must be present, finite and satisfy the requested real window", () => {
  const { config, report } = fixture();
  for (const value of [undefined, NaN, Infinity, -1]) {
    const invalid = structuredClone(report);
    invalid.measured_idle_seconds = value;
    assert.throws(() => validateReport(invalid, config));
  }
  assert.throws(() => validateReport(report, { ...config, idle: 3600 }));
});
const workflow = fs
  .readFileSync(".github/workflows/lifecycle-soak.yml", "utf8")
  .replaceAll("\r\n", "\n");
// Deliberately validate this small manual-job contract without an npm YAML
// dependency: the workflow runs these checks before dependency installation.
function checkManualWorkflow(source) {
  const workflow = source.replace(/^\s*#.*$/gm, "");
  assert.match(workflow, /workflow_dispatch:/);
  assert.doesNotMatch(
    workflow,
    /pull_request:|push:|schedule:|secrets\.|npm ci|tauri dev/,
  );
  assert.match(workflow, /contents: read/);
  assert.match(workflow, /^ {4}runs-on: windows-2022$/m);
  assert.doesNotMatch(
    workflow,
    /^ {4}(?:if|needs|continue-on-error|environment):/m,
  );
  assert.doesNotMatch(
    workflow,
    /continue-on-error:|--features (?:cuda|vulkan)|NODE_AUTH_TOKEN|FONO_PREPARING_RELEASE_RESOURCES/,
  );
  const config = JSON.parse(
    workflow.match(/^ {6}TAURI_CONFIG: '([^']+)'$/m)?.[1] ?? "null",
  );
  assert.deepEqual(config, {
    build: { frontendDist: "../scripts/ci-native-frontend" },
    bundle: {
      resources: { "resources/stt-base/fono-stt-vulkan-worker.exe": null },
    },
  });
  const starts = [...workflow.matchAll(/^ {6}- (?:name|uses): /gm)];
  const step = (name) => {
    const index = starts.findIndex((match) =>
      workflow.startsWith(`      - name: ${name}\n`, match.index),
    );
    assert.ok(index >= 0, `missing mandatory soak step: ${name}`);
    return workflow.slice(starts[index].index, starts[index + 1]?.index);
  };
  const critical = [
    "Verify explicit soak runner and evidence guards",
    "Prepare verified CPU resources",
    "Build real CPU helper and ignored native test executable",
    "Stage verified native VAD DLLs beside test executable",
    "Run explicit mock-audio and real CPU-worker lifecycle soak",
  ];
  for (const name of critical)
    assert.doesNotMatch(step(name), /^\s+(?:if|continue-on-error):/m);
  assert.match(
    step(critical[0]),
    /run: node --test scripts\/lifecycle-soak\.test\.mjs\n/,
  );
  assert.match(
    step(critical[1]),
    /run: \.\/scripts\/bootstrap-ci-resources\.ps1\n/,
  );
  const build = step(critical[2]);
  assert.match(
    build,
    /^ {10}cargo build --locked --release --no-default-features -p fono-stt-worker --bin fono-stt-worker$/m,
  );
  assert.match(
    build,
    /^ {10}cargo test --locked -p fono --lib --no-default-features --features cpu-only --no-run --message-format=json > \.\.\/build\/lifecycle-soak\/tests\.jsonl$/m,
  );
  assert.equal(
    [
      ...build.matchAll(
        /if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}/g,
      ),
    ].length,
    2,
  );
  assert.match(
    step(critical[3]),
    /run: node scripts\/native-test-runtime\.mjs build\/lifecycle-soak\/tests\.jsonl \$env:SHERPA_ONNX_LIB_DIR \$env:CARGO_TARGET_DIR\n/,
  );
  assert.match(
    step(critical[4]),
    /run: \.\/scripts\/lifecycle-soak\.ps1 -Cycles \$env:SOAK_CYCLES -Faults \$env:SOAK_FAULTS -IdleSeconds \$env:SOAK_IDLE_SECONDS -DownloadModel\n/,
  );
  const artifact = step(
    "Save native measurements, runner receipt and failure log",
  );
  assert.match(artifact, /^ {8}if: always\(\)$/m);
  assert.match(artifact, /uses: actions\/upload-artifact@[0-9a-f]{40}/);
  for (const name of [
    "report.json",
    "report.json.runner.json",
    "report.json.log",
  ])
    assert.ok(artifact.includes(`            build/lifecycle-soak/${name}\n`));
}
test("manual workflow keeps CPU-only inputs, inert frontend, mandatory execution and evidence", () => {
  checkManualWorkflow(workflow);
});
test("manual workflow contract rejects skipped work, GPU/default drift and lost evidence", () => {
  const mutations = [
    (s) =>
      s.replace(
        "cargo build --locked --release --no-default-features",
        "cargo build --locked --release",
      ),
    (s) =>
      s.replace(
        "-p fono-stt-worker --bin fono-stt-worker",
        "--features cuda -p fono-stt-worker --bin fono-stt-worker",
      ),
    (s) => s.replace(/^ {6}TAURI_CONFIG:.*\n/m, ""),
    (s) => s.replace("../scripts/ci-native-frontend", "../dist"),
    (s) =>
      s.replace(
        / {6}- name: Stage verified native VAD DLLs[\s\S]*?(?= {6}- name:)/,
        "",
      ),
    (s) => s.replace(/ {6}- name: Save native measurements,[\s\S]*/, ""),
    (s) =>
      s.replace(
        "            build/lifecycle-soak/report.json.runner.json\n",
        "",
      ),
    (s) =>
      s.replace(
        "      - name: Build real CPU helper and ignored native test executable\n",
        "      - name: Build real CPU helper and ignored native test executable\n        continue-on-error: true\n",
      ),
    (s) =>
      s.replace(
        "      - name: Run explicit mock-audio and real CPU-worker lifecycle soak\n",
        "      - name: Run explicit mock-audio and real CPU-worker lifecycle soak\n        if: false\n",
      ),
    (s) => s.replace("  lifecycle:\n", "  lifecycle:\n    if: false\n"),
    (s) => s.replace("        if: always()", "        if: success()"),
  ];
  for (const mutate of mutations) {
    const changed = mutate(workflow);
    assert.notEqual(changed, workflow, "mutation must alter the real workflow");
    assert.throws(() => checkManualWorkflow(changed));
  }
});
