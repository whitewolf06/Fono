import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../", import.meta.url));
const pwsh = spawnSync(
  "pwsh",
  ["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"],
  {
    encoding: "utf8",
    windowsHide: true,
  },
);
const windowsPowerShell = process.platform === "win32" && pwsh.status === 0;
const pythonDiscovery = windowsPowerShell
  ? spawnSync(
      "pwsh",
      [
        "-NoProfile",
        "-Command",
        "(Get-Command python.exe -CommandType Application -All -ErrorAction SilentlyContinue | Where-Object { $_.Source -notmatch '[\\/]WindowsApps[\\/]' } | Select-Object -First 1).Source",
      ],
      { encoding: "utf8", windowsHide: true },
    )
  : undefined;
const pythonExecutable =
  pythonDiscovery?.status === 0 ? pythonDiscovery.stdout.trim() : "";
const pythonVersion = pythonExecutable
  ? spawnSync(
      pythonExecutable,
      ["-I", "-c", "import sys; sys.exit(sys.version_info < (3, 12))"],
      { windowsHide: true },
    )
  : undefined;
const windowsPython = windowsPowerShell && pythonVersion?.status === 0;

function fixture(t, bootstrapFails = false) {
  const fixtureRoot = path.join(repoRoot, "build/cpu-launcher-tests");
  mkdirSync(fixtureRoot, { recursive: true });
  const directory = mkdtempSync(path.join(fixtureRoot, "checkout space "));
  for (const relative of [
    "scripts",
    "src-tauri",
    "node_modules/@tauri-apps/cli",
    "runtime lib",
  ]) {
    mkdirSync(path.join(directory, relative), { recursive: true });
  }
  copyFileSync(
    path.join(repoRoot, "scripts/dev-cpu.ps1"),
    path.join(directory, "scripts/dev-cpu.ps1"),
  );
  copyFileSync(
    path.join(repoRoot, "src-tauri/tauri.cpu.conf.json"),
    path.join(directory, "src-tauri/tauri.cpu.conf.json"),
  );
  writeFileSync(
    path.join(directory, "scripts/bootstrap-ci-resources.ps1"),
    `
[CmdletBinding()]
param([switch]$InstallGpuSdks)
if ($InstallGpuSdks) { throw 'fixture must not install GPU SDKs' }
$root = Split-Path -Parent $PSScriptRoot
[IO.File]::WriteAllText((Join-Path $root 'bootstrap-called'), 'yes')
$env:SHERPA_ONNX_LIB_DIR = Join-Path $root 'runtime lib'
$env:PATH = "$($env:SHERPA_ONNX_LIB_DIR);$env:PATH"
${bootstrapFails ? "throw 'fixture bootstrap refused'" : ""}
`,
  );
  writeFileSync(
    path.join(directory, "node_modules/@tauri-apps/cli/tauri.js"),
    `
const fs = require('node:fs');
fs.writeFileSync(process.env.FONO_CPU_CAPTURE, JSON.stringify({
  args: process.argv.slice(2), cwd: process.cwd(),
  sherpa: process.env.SHERPA_ONNX_LIB_DIR, path: process.env.PATH,
  bindings: process.env.WHISPER_DONT_GENERATE_BINDINGS,
  cuda: process.env.GGML_CUDA, vulkan: process.env.GGML_VULKAN,
  oldCublas: process.env.WHISPER_CUBLAS,
  oldCuda: process.env.WHISPER_CUDA, oldVulkan: process.env.WHISPER_VULKAN,
  preparing: process.env.FONO_PREPARING_RELEASE_RESOURCES,
  tauriConfig: process.env.TAURI_CONFIG,
  profile: process.env.FONO_TEST_DATA_DIR, port: process.env.FONO_API_PORT
}));
process.exit(Number(process.env.FONO_CPU_FAKE_EXIT || 0));
`,
  );
  t.after(() => {
    const temporary = realpathSync(fixtureRoot);
    const resolved = realpathSync(directory);
    const relative = path.relative(temporary, resolved);
    assert.ok(
      relative && !relative.startsWith("..") && !path.isAbsolute(relative),
    );
    rmSync(resolved, { recursive: true, force: true });
  });
  const capture = path.join(directory, "launch.json");
  return {
    directory,
    capture,
    run(overrides = {}) {
      return spawnSync(
        "pwsh",
        ["-NoProfile", "-File", path.join(directory, "scripts/dev-cpu.ps1")],
        {
          cwd: os.tmpdir(),
          encoding: "utf8",
          windowsHide: true,
          env: {
            ...process.env,
            FONO_CPU_CAPTURE: capture,
            FONO_TEST_DATA_DIR: "",
            FONO_API_PORT: "",
            GITHUB_ENV: "",
            GITHUB_PATH: "",
            ...overrides,
          },
        },
      );
    },
  };
}

test(
  "CPU launcher keeps bootstrap environment, uses real native profile and forwards only Cargo flags",
  { skip: !windowsPowerShell },
  (t) => {
    const setup = fixture(t);
    const result = setup.run({
      GGML_CUDA: "ON",
      GGML_VULKAN: "ON",
      WHISPER_CUBLAS: "ON",
      WHISPER_CUDA: "ON",
      WHISPER_VULKAN: "ON",
      WHISPER_DONT_GENERATE_BINDINGS: "1",
      FONO_PREPARING_RELEASE_RESOURCES: "1",
      TAURI_CONFIG: '{"build":{"devUrl":"http://wrong-preview.invalid"}}',
    });
    assert.equal(result.status, 0, result.stderr);
    assert.ok(existsSync(path.join(setup.directory, "bootstrap-called")));
    const actual = JSON.parse(readFileSync(setup.capture, "utf8"));
    assert.deepEqual(actual.args, [
      "dev",
      "--config",
      path.join(setup.directory, "src-tauri/tauri.cpu.conf.json"),
      "--features",
      "cpu-only",
      "--",
      "--no-default-features",
      "--locked",
    ]);
    assert.equal(actual.cwd, setup.directory);
    assert.equal(actual.sherpa, path.join(setup.directory, "runtime lib"));
    assert.ok(actual.path.startsWith(`${actual.sherpa};`));
    assert.equal(actual.bindings, undefined);
    assert.equal(actual.cuda, "OFF");
    assert.equal(actual.vulkan, "OFF");
    assert.equal(actual.oldCublas, undefined);
    assert.equal(actual.oldCuda, undefined);
    assert.equal(actual.oldVulkan, undefined);
    assert.equal(actual.preparing, undefined);
    assert.equal(actual.tauriConfig, undefined);
    assert.equal(
      actual.profile,
      path.join(setup.directory, "build/dev-cpu-profile"),
    );
    assert.equal(actual.port, "17833");
  },
);

test(
  "CPU launcher preserves explicit isolated profile/port and returns the child failure",
  { skip: !windowsPowerShell },
  (t) => {
    const setup = fixture(t);
    const profile = path.join(setup.directory, "chosen profile");
    const result = setup.run({
      FONO_TEST_DATA_DIR: profile,
      FONO_API_PORT: "41081",
      FONO_CPU_FAKE_EXIT: "17",
    });
    assert.equal(result.status, 17, result.stderr);
    const actual = JSON.parse(readFileSync(setup.capture, "utf8"));
    assert.equal(actual.profile, profile);
    assert.equal(actual.port, "41081");
  },
);

test(
  "failed verified bootstrap prevents any Tauri or Cargo launch",
  { skip: !windowsPowerShell },
  (t) => {
    const setup = fixture(t, true);
    const result = setup.run();
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /fixture bootstrap refused/);
    assert.equal(existsSync(setup.capture), false);
  },
);

test(
  "relative profile is rejected before downloading dependencies or launching native code",
  { skip: !windowsPowerShell },
  (t) => {
    const setup = fixture(t);
    const result = setup.run({ FONO_TEST_DATA_DIR: "relative-profile" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /FONO_TEST_DATA_DIR must be absolute/);
    assert.equal(
      existsSync(path.join(setup.directory, "bootstrap-called")),
      false,
    );
    assert.equal(existsSync(setup.capture), false);
  },
);

test(
  "CPU launcher conditionally runs the existing cache guards and preserves the child exit code",
  { skip: !windowsPowerShell },
  (t) => {
    const setup = fixture(t);
    const calls = path.join(setup.directory, "maintenance.jsonl");
    writeFileSync(
      path.join(setup.directory, "scripts/clean-build-cache.mjs"),
      `
import fs from 'node:fs';
fs.appendFileSync(${JSON.stringify(calls)}, JSON.stringify(process.argv.slice(2)) + '\\n');
`,
    );
    const result = setup.run({ FONO_CPU_FAKE_EXIT: "17" });
    assert.equal(result.status, 17, result.stderr);
    assert.deepEqual(
      readFileSync(calls, "utf8").trim().split("\n").map(JSON.parse),
      [
        ["--apply", "--if-needed"],
        ["--apply", "--if-needed"],
      ],
    );
  },
);

function prepareVerifiedBootstrap(setup) {
  for (const name of [
    "bootstrap-ci-resources.ps1",
    "extract-ci-archive.py",
    "extract-ci-archive.test.py",
  ]) {
    copyFileSync(
      path.join(repoRoot, "scripts", name),
      path.join(setup.directory, "scripts", name),
    );
  }
  const work = path.join(setup.directory, "build/ci-dependencies");
  mkdirSync(work, { recursive: true });
  const archive = path.join(work, "fixture.tar.bz2");
  const created = spawnSync(
    pythonExecutable,
    [
      "-I",
      "-B",
      "-c",
      `
import io, pathlib, sys, tarfile
with tarfile.open(pathlib.Path(sys.argv[1]), 'w:bz2') as archive:
    for name in ('onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll', 'sherpa-onnx-c-api.lib', 'onnxruntime.lib'):
        content = ('trusted fixture ' + name).encode()
        member = tarfile.TarInfo('sherpa-fixture/lib/' + name)
        member.size = len(content)
        archive.addfile(member, io.BytesIO(content))
`,
      archive,
    ],
    { encoding: "utf8", windowsHide: true },
  );
  assert.equal(created.status, 0, created.stderr);
  const sha256 = createHash("sha256")
    .update(readFileSync(archive))
    .digest("hex");
  const vad = path.join(
    setup.directory,
    "src-tauri/resources/vad/fixture.onnx",
  );
  mkdirSync(path.dirname(vad), { recursive: true });
  writeFileSync(vad, "fixture VAD");
  writeFileSync(
    path.join(setup.directory, "src-tauri/Cargo.lock"),
    '[[package]]\nname = "sherpa-onnx-sys"\nversion = "1.13.4"\n',
  );
  writeFileSync(
    path.join(setup.directory, "scripts/ci-native-dependencies.json"),
    JSON.stringify({
      sherpa: {
        version: "1.13.4",
        url: "https://fixture.invalid/fixture.tar.bz2",
        sha256,
        directory: "sherpa-fixture",
        downloadTimeoutSeconds: 30,
      },
      vad: {
        path: "src-tauri/resources/vad/fixture.onnx",
        sha256: createHash("sha256").update(readFileSync(vad)).digest("hex"),
      },
    }),
  );
  return { work, sha256 };
}

test(
  "verified bootstrap reuses one immutable cache across successful startups and blocks tampered DLLs",
  { skip: !windowsPython },
  (t) => {
    const setup = fixture(t);
    const { work, sha256 } = prepareVerifiedBootstrap(setup);
    const first = setup.run();
    assert.equal(first.status, 0, first.stderr + first.stdout);
    const cached = path.join(
      work,
      `sherpa-verified-${sha256}`,
      "sherpa-fixture/lib/onnxruntime.dll",
    );
    const before = statSync(cached);
    const second = setup.run();
    assert.equal(second.status, 0, second.stderr + second.stdout);
    assert.match(second.stdout, /every pinned archive file; reusing/);
    assert.equal(statSync(cached).mtimeMs, before.mtimeMs);
    assert.equal(statSync(cached).ino, before.ino);
    assert.equal(
      readdirSync(work, { withFileTypes: true }).filter((entry) =>
        entry.isDirectory(),
      ).length,
      1,
    );
    assert.equal(
      JSON.parse(readFileSync(setup.capture, "utf8")).sherpa,
      path.dirname(cached),
    );
    const content = readFileSync(cached);
    content[0] ^= 1;
    writeFileSync(cached, content);
    rmSync(setup.capture);
    const tampered = setup.run();
    assert.notEqual(tampered.status, 0);
    assert.match(tampered.stdout + tampered.stderr, /cache SHA-256 differs/);
    assert.equal(existsSync(setup.capture), false);
    assert.deepEqual(readFileSync(cached), content);
  },
);

for (const linkedPath of ["build parent", "resource parent", "DLL leaf"]) {
  test(
    `verified bootstrap rejects a junction in the ${linkedPath} before any outside writes`,
    { skip: !windowsPython },
    (t) => {
      const setup = fixture(t);
      prepareVerifiedBootstrap(setup);
      const outside = fixture(t);
      const target = path.join(outside.directory, "linked target");
      let linked;
      if (linkedPath === "build parent") {
        linked = path.join(setup.directory, "build");
        renameSync(linked, target);
      } else if (linkedPath === "resource parent") {
        linked = path.join(setup.directory, "src-tauri/resources");
        renameSync(linked, target);
      } else {
        linked = path.join(
          setup.directory,
          "src-tauri/resources/sherpa-onnx/onnxruntime.dll",
        );
        mkdirSync(path.dirname(linked), { recursive: true });
        mkdirSync(target);
      }
      writeFileSync(
        path.join(target, "must-remain.txt"),
        "owned outside the checkout",
      );
      const before = readdirSync(target, { recursive: true }).sort();
      symlinkSync(target, linked, "junction");
      let result;
      try {
        result = setup.run();
      } finally {
        // Remove only our own junction, never recurse through its target.
        unlinkSync(linked);
      }
      assert.notEqual(result.status, 0);
      assert.match(result.stdout + result.stderr, /reparse point/);
      assert.equal(existsSync(setup.capture), false);
      assert.deepEqual(readdirSync(target, { recursive: true }).sort(), before);
      assert.equal(
        readFileSync(path.join(target, "must-remain.txt"), "utf8"),
        "owned outside the checkout",
      );
    },
  );
}

test("CPU Tauri patch removes GPU resources and retains real Sherpa/VAD and native frontend", () => {
  const base = JSON.parse(
    readFileSync(path.join(repoRoot, "src-tauri/tauri.conf.json"), "utf8"),
  );
  const cpu = JSON.parse(
    readFileSync(path.join(repoRoot, "src-tauri/tauri.cpu.conf.json"), "utf8"),
  );
  const resources = { ...base.bundle.resources, ...cpu.bundle.resources };
  for (const [key, value] of Object.entries(resources))
    if (value === null) delete resources[key];
  assert.equal(resources["resources/stt-workers/*"], undefined);
  assert.equal(resources["resources/stt-base/fono-stt-vulkan-worker.exe"], undefined);
  assert.equal(resources["resources/sherpa-onnx/*.dll"], "");
  assert.equal(resources["resources/vad/*.onnx"], "resources/vad/");
  assert.equal(resources["resources/whisper/*"], "resources/whisper/");
  assert.equal(cpu.build.beforeBuildCommand, "npm run build");
  assert.equal(cpu.build.beforeDevCommand, undefined);
  assert.equal(base.build.beforeDevCommand, "npm run dev");
  assert.equal(cpu.app, undefined);
});
