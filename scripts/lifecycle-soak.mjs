// CI-04: launch one explicit ignored native subsystem soak, without Cargo or UI.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import os from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";
import { modelSpec } from "./stt-quality.mjs";

export const SOAK_TEST =
  "stt::worker::lifecycle_soak::cpu_worker_lifecycle_soak";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const corpus = path.join(root, "src-tauri/tests/fixtures/stt-quality");
export function digest(file) {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}
export function limits(options) {
  const cycles = integer(options["--cycles"], 1000, 100_000);
  const faults = integer(options["--faults"], 100, 10_000);
  const active = integer(
    options["--active-cancel-every"],
    Math.min(100, cycles),
    cycles,
  );
  const idle = integer(options["--idle-seconds"], 0, 7200, 0);
  const timeout = integer(options["--timeout-seconds"], 7200, 14_400);
  if (timeout <= idle)
    throw new Error("Timeout must exceed the requested idle window");
  return { cycles, faults, active, idle, timeout };
}
function integer(value, fallback, maximum, minimum = 1) {
  const parsed = value === undefined ? fallback : Number(value);
  if (
    !Number.isSafeInteger(parsed) ||
    parsed < minimum ||
    parsed > maximum ||
    (value !== undefined && !/^\d+$/.test(String(value)))
  )
    throw new Error(`Expected integer ${minimum}..${maximum}`);
  return parsed;
}
function regular(file, expectedHash, executable = false) {
  if (!file || !path.isAbsolute(file))
    throw new Error("An explicit absolute input path is required");
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink())
    throw new Error("Expected a regular input file, without a symlink");
  if (executable && path.extname(file).toLowerCase() !== ".exe")
    throw new Error("Expected an explicit Windows EXE");
  if (!/^[0-9a-f]{64}$/.test(expectedHash ?? ""))
    throw new Error("An explicit SHA256 is required");
  if (digest(file) !== expectedHash)
    throw new Error("Input SHA256 differs from the explicit expected hash");
  return file;
}
export function prepare(options, platform = process.platform) {
  if (platform !== "win32")
    throw new Error("Native Win32 lifecycle metrics require Windows");
  const allowed = new Set([
    "--runner",
    "--runner-sha256",
    "--worker",
    "--worker-sha256",
    "--model",
    "--wav",
    "--report",
    "--cycles",
    "--faults",
    "--active-cancel-every",
    "--idle-seconds",
    "--timeout-seconds",
    "--worker-profile",
  ]);
  for (const key of Object.keys(options))
    if (!allowed.has(key))
      throw new Error(`Unknown lifecycle-soak option: ${key}`);
  const runner = regular(options["--runner"], options["--runner-sha256"], true);
  const worker = regular(options["--worker"], options["--worker-sha256"], true);
  if (fs.realpathSync(runner) === fs.realpathSync(worker))
    throw new Error(
      "Native test driver and CPU worker must be different binaries",
    );
  const model = regular(options["--model"], modelSpec.sha256);
  const manifest = JSON.parse(
    fs.readFileSync(path.join(corpus, "manifest.json"), "utf8"),
  );
  const wav = options["--wav"] ?? path.join(corpus, manifest.entries[0].path);
  const entry = manifest.entries.find(
    (entry) => path.resolve(corpus, entry.path) === path.resolve(wav),
  );
  if (!entry)
    throw new Error("WAV must be a committed, licensed public-corpus entry");
  regular(wav, entry.sha256);
  const { cycles, faults, active, idle, timeout } = limits(options);
  const report = path.resolve(
    options["--report"] ?? path.join(root, "build/lifecycle-soak/report.json"),
  );
  if (
    [runner, worker, model, wav].some((input) => path.resolve(input) === report)
  )
    throw new Error("Report cannot overwrite a verified input");
  return {
    runner,
    worker,
    model,
    wav,
    report,
    cycles,
    faults,
    active,
    idle,
    timeout,
    runnerSha256: options["--runner-sha256"],
    workerSha256: options["--worker-sha256"],
    workerProfile: options["--worker-profile"] ?? "unspecified by invoker",
    wavSha256: entry.sha256,
    corpus: {
      id: entry.id,
      license: manifest.license,
      attribution: manifest.attribution,
    },
  };
}
export function parseArgs(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (
      !key?.startsWith("--") ||
      value === undefined ||
      value.startsWith("--") ||
      key in options
    )
      throw new Error("Options require one distinct key and one value");
    options[key] = value;
  }
  return options;
}
export function validateReport(report, config) {
  if (
    report?.schema !== 1 ||
    report.kind !== "fono-production-subsystem-lifecycle-soak"
  )
    throw new Error("Missing native subsystem report");
  const exact = [
    [report.limits?.start_cancel_cycles, config.cycles],
    [report.limits?.worker_faults, config.faults],
    [report.completed?.start_cancel_cycles, config.cycles],
    [report.completed?.fault_kills, config.faults],
    [report.completed?.fault_reaps, config.faults],
    [report.completed?.fault_restarts, config.faults],
    [report.inputs?.worker_sha256, config.workerSha256],
    [report.inputs?.model_sha256, modelSpec.sha256],
    [report.inputs?.wav_sha256, config.wavSha256],
  ];
  if (exact.some(([actual, expected]) => actual !== expected))
    throw new Error(
      "Native report counts or hashes do not match the requested soak",
    );
  if (
    report.status !== "passed" ||
    report.completed.all_owned_children_exited !== true ||
    !Array.isArray(report.resource_samples) ||
    !report.resource_samples.some((entry) => entry.phase === "warm_baseline") ||
    !report.resource_samples.some(
      (entry) => entry.phase === "final_shutdown",
    ) ||
    !Number.isFinite(report.measured_idle_seconds) ||
    report.measured_idle_seconds < 0 ||
    report.measured_idle_seconds < config.idle
  )
    throw new Error(
      "Native soak did not complete the measured resource lifecycle",
    );
  for (const entry of report.resource_samples) {
    for (const sample of [
      entry.parent,
      ...(entry.worker ? [entry.worker] : []),
    ]) {
      if (
        !sample ||
        ["pid", "handles", "threads", "private_bytes"].some(
          (key) => !Number.isSafeInteger(sample[key]) || sample[key] <= 0,
        ) ||
        !Number.isFinite(sample.cpu_seconds) ||
        sample.cpu_seconds < 0
      )
        throw new Error(
          "Native resource sample has missing or invalid numeric measurements",
        );
    }
  }
  if (
    !report.resource_samples.some(
      (entry) => entry.phase === "cold_pre_worker",
    ) ||
    !report.resource_samples.some(
      (entry) => entry.phase === "warm_no_worker_baseline",
    ) ||
    !report.resource_samples.some((entry) => entry.phase === "final_loaded")
  )
    throw new Error("Native report is missing comparable resource phases");
  if (
    !Array.isArray(report.worker_faults) ||
    report.worker_faults.length !== config.faults ||
    report.worker_faults.some(
      (fault) =>
        fault.alive_before_kill !== true ||
        fault.confirmed_exit !== true ||
        !Number.isSafeInteger(fault.owned_pid),
    )
  )
    throw new Error("Missing individual owned worker kill/reap receipts");
  for (const key of [
    "capture_start",
    "capture_stop",
    "scheduler_acquire_uncontended",
    "cancel_to_request_return",
  ]) {
    const latency = report.latency_ms?.[key];
    if (
      latency?.count !== config.cycles ||
      ["p50", "p95", "p99"].some(
        (q) => !Number.isFinite(latency[q]) || latency[q] < 0,
      )
    )
      throw new Error("Missing measured per-cycle latency distribution");
  }
  for (const phase of ["loaded_parent", "shutdown_parent"])
    for (const key of ["handles_pass", "threads_pass", "private_bytes_pass"])
      if (report.resource_gates?.[phase]?.[key] !== true)
        throw new Error("Native resource gate did not pass");
  const phaseSample = (phase) =>
    report.resource_samples.find((entry) => entry.phase === phase);
  for (const [before, after] of [
    [phaseSample("warm_baseline").parent, phaseSample("final_loaded").parent],
    [
      phaseSample("warm_no_worker_baseline").parent,
      phaseSample("final_shutdown").parent,
    ],
  ]) {
    if (
      after.handles > before.handles ||
      after.threads > before.threads ||
      after.private_bytes >
        before.private_bytes +
          Math.max(Math.floor(before.private_bytes / 20), 10 * 1024 * 1024)
    )
      throw new Error(
        "Raw process measurements contradict the declared passing resource budget",
      );
  }
  for (const phase of ["warm_baseline", "final_loaded"])
    if (!phaseSample(phase).worker || phaseSample(phase).worker.threads <= 0)
      throw new Error("Missing live owned CPU-worker resource sample");
  const active = Math.floor(config.cycles / config.active);
  if (
    report.completed.active_worker_cancellations !== active ||
    report.completed.pretransport_worker_cancellations !==
      config.cycles - active
  )
    throw new Error(
      "Cancellation accounting does not match actual requested phases",
    );
  return report;
}
export async function lifecycleSoak(options) {
  const config = prepare(options);
  fs.mkdirSync(path.dirname(config.report), { recursive: true });
  if (fs.existsSync(config.report))
    throw new Error(
      "Use a fresh report path; existing evidence is never overwritten",
    );
  const envelope = `${config.report}.runner.json`;
  if (fs.existsSync(envelope)) throw new Error("Runner receipt already exists");
  const logPath = `${config.report}.log`;
  if (fs.existsSync(logPath)) throw new Error("Soak log already exists");
  // Open the exclusive log before spawning a process. A stale log or an I/O
  // failure cannot leave a native driver running without a controlling runner.
  const logDescriptor = fs.openSync(logPath, "wx");
  const started = new Date().toISOString();
  const child = spawn(
    config.runner,
    [SOAK_TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
    {
      cwd: root,
      shell: false,
      windowsHide: true,
      env: {
        ...process.env,
        FONO_SOAK_WORKER: config.worker,
        FONO_SOAK_WORKER_SHA256: config.workerSha256,
        FONO_SOAK_MODEL: config.model,
        FONO_SOAK_WAV: config.wav,
        FONO_SOAK_WAV_SHA256: config.wavSha256,
        FONO_SOAK_REPORT: config.report,
        FONO_SOAK_CYCLES: String(config.cycles),
        FONO_SOAK_FAULTS: String(config.faults),
        FONO_SOAK_ACTIVE_CANCEL_EVERY: String(config.active),
        FONO_SOAK_IDLE_SECONDS: String(config.idle),
        FONO_SOAK_RUNNER_PID: String(process.pid),
        FONO_SOAK_TIMEOUT_SECONDS: String(config.timeout),
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  let interrupted = false;
  let timedOut = false;
  const stop = () => {
    interrupted = true;
    child.kill();
  };
  // The native test joins a private kill-on-close job before creating a worker;
  // terminating this exact owned driver closes the job and kills descendants.
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  const timer = setTimeout(() => {
    timedOut = true;
    child.kill();
  }, config.timeout * 1000);
  const log = fs.createWriteStream(logPath, {
    fd: logDescriptor,
    autoClose: true,
  });
  log.on("error", () => {
    interrupted = true;
    child.kill();
  });
  child.stdout.pipe(log, { end: false });
  child.stderr.pipe(log, { end: false });
  let result;
  try {
    result = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => resolve({ code, signal }));
    });
  } finally {
    clearTimeout(timer);
    process.off("SIGINT", stop);
    process.off("SIGTERM", stop);
    await new Promise((resolve) => log.end(resolve));
    fs.writeFileSync(
      envelope,
      JSON.stringify(
        {
          schema: 1,
          started_at: started,
          finished_at: new Date().toISOString(),
          driver_sha256: config.runnerSha256,
          worker_sha256: config.workerSha256,
          worker_profile: config.workerProfile,
          owned_driver_pid: child.pid,
          exact_test: SOAK_TEST,
          requested: {
            cycles: config.cycles,
            faults: config.faults,
            active_cancel_every: config.active,
            idle_seconds: config.idle,
          },
          corpus: config.corpus,
          hardware: {
            platform: process.platform,
            architecture: process.arch,
            os_release: os.release(),
            logical_cpus: os.cpus().length,
            cpu_models: [...new Set(os.cpus().map((cpu) => cpu.model))],
            total_memory_bytes: os.totalmem(),
          },
          tools: { node: process.versions.node },
          interrupted,
          timed_out: timedOut,
          result,
          scope:
            "production native subsystems with mock physical audio; not desktop, automatic recovery router or STAB-06 acceptance",
        },
        null,
        2,
      ),
      { flag: "wx" },
    );
  }
  if (interrupted || timedOut || result?.code !== 0)
    throw new Error(
      `Native lifecycle soak failed; inspect ${path.basename(config.report)}.log`,
    );
  const report = JSON.parse(fs.readFileSync(config.report, "utf8"));
  validateReport(report, config);
  console.log(
    `CPU lifecycle soak: ${config.cycles} start/cancel, ${config.faults} real worker kill/reap/restarts; measured resources passed.`,
  );
  return report;
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  lifecycleSoak(parseArgs(process.argv.slice(2))).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
