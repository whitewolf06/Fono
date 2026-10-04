import assert from "node:assert/strict";
import {
  mkdtemp,
  mkdir,
  readFile,
  readdir,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  RUNTIME_DLLS,
  stageTestRuntime,
  testExecutables,
} from "./native-test-runtime.mjs";

async function fixture(run) {
  const directory = await mkdtemp(join(tmpdir(), "fono-native-runtime-test-"));
  try {
    const target = join(directory, "custom-target");
    const runtime = join(directory, "verified-runtime");
    await mkdir(target);
    await mkdir(runtime);
    for (const name of RUNTIME_DLLS) {
      await writeFile(join(runtime, name), `verified DLL fixture: ${name}`);
    }
    await run({ directory, target, runtime });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function executable(directory, name = "fono_wake-test.exe") {
  await mkdir(directory, { recursive: true });
  const path = join(directory, name);
  await writeFile(path, "test artifact fixture; never executed");
  return path;
}

function messages(executables, success = true) {
  return [
    ...executables.map((executable) => ({
      reason: "compiler-artifact",
      executable,
    })),
    { reason: "build-finished", success },
  ]
    .map((message) => JSON.stringify(message))
    .join("\n");
}

test("stages all four linked DLLs at exact Cargo paths and replaces stale copies", async () => {
  await fixture(async ({ target, runtime }) => {
    const deps = join(target, "debug", "deps");
    const crossDeps = join(target, "x86_64-pc-windows-msvc", "release", "deps");
    const artifacts = [await executable(deps), await executable(crossDeps)];
    await writeFile(
      join(deps, "onnxruntime.dll"),
      "incompatible old system runtime",
    );
    await writeFile(join(deps, "unrelated.txt"), "retain this file");
    const selected = await testExecutables(messages(artifacts), target);
    await stageTestRuntime(selected, runtime, () => {});
    for (const directory of [deps, crossDeps]) {
      for (const name of RUNTIME_DLLS) {
        assert.deepEqual(
          await readFile(join(directory, name)),
          await readFile(join(runtime, name)),
        );
      }
      assert.ok(
        !(await readdir(directory)).some((name) => name.endsWith(".part")),
      );
    }
    assert.equal(
      await readFile(join(deps, "unrelated.txt"), "utf8"),
      "retain this file",
    );
    await stageTestRuntime(selected, runtime, () => {});
  });
});

test("rejects failed builds and artifacts outside the selected target", async () => {
  await fixture(async ({ directory, target }) => {
    const artifact = await executable(join(target, "debug", "deps"));
    const outside = await executable(join(directory, "outside"));
    await assert.rejects(
      testExecutables(messages([artifact], false), target),
      /finish successfully/,
    );
    await assert.rejects(
      testExecutables(messages([outside]), target),
      /outside/,
    );
    await assert.rejects(
      testExecutables(messages([]), target),
      /test executables/,
    );
    await assert.rejects(
      testExecutables(messages(["relative.exe"]), target),
      /Invalid/,
    );
  });
});

test("a linked artifact directory cannot redirect writes outside the target", async () => {
  await fixture(async ({ directory, target }) => {
    const outsideDirectory = join(directory, "outside");
    await executable(outsideDirectory);
    const link = join(target, "redirect");
    await symlink(outsideDirectory, link, "junction");
    await assert.rejects(
      testExecutables(messages([join(link, "fono_wake-test.exe")]), target),
      /outside/,
    );
    assert.deepEqual(await readdir(outsideDirectory), ["fono_wake-test.exe"]);
  });
});

test("an incomplete runtime is rejected before changing any existing DLL", async () => {
  await fixture(async ({ target, runtime }) => {
    const deps = join(target, "debug", "deps");
    const artifact = await executable(deps);
    await writeFile(join(deps, "onnxruntime.dll"), "old runtime");
    await rm(join(runtime, RUNTIME_DLLS.at(-1)));
    await assert.rejects(
      stageTestRuntime([artifact], runtime, () => {}),
      /ENOENT/,
    );
    assert.equal(
      await readFile(join(deps, "onnxruntime.dll"), "utf8"),
      "old runtime",
    );
  });
});
