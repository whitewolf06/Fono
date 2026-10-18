import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  checkFrontendDependencyLock,
  checkInstalledRollup,
} from "./ci-frontend-dependencies.mjs";

const lock = JSON.parse(
  await readFile(new URL("../package-lock.json", import.meta.url), "utf8"),
);
const target = "@rollup/rollup-linux-x64-gnu";
const path = `node_modules/${target}`;
const copy = () => structuredClone(lock);

function checkFrontendWorkflow(source) {
  const job = source
    .replaceAll("\r\n", "\n")
    .match(/^ {2}frontend:\n([\s\S]*?)(?=^ {2}\w[\w-]*:\n|$(?![\s\S]))/m)?.[1];
  assert.ok(job, "frontend job must exist");
  assert.doesNotMatch(
    job,
    /^ {4}(?:if|needs|environment):/m,
    "forks must run frontend unconditionally",
  );
  assert.doesNotMatch(
    job,
    /continue-on-error:/,
    "dependency failure must fail the job",
  );
  const steps = job.split(/^ {6}- /m).slice(1);
  const commands = [
    "node scripts/ci-frontend-dependencies.mjs --lock",
    "npm ci",
    "node scripts/ci-frontend-dependencies.mjs --installed",
    "node --test scripts/ci-frontend-dependencies.test.mjs",
  ];
  const positions = commands.map((command) => {
    const index = steps.findIndex((step) =>
      step
        .split("\n")
        .some((line) => line.trim().replace(/^run: /, "") === command),
    );
    assert.notEqual(index, -1, `missing dependency gate: ${command}`);
    assert.doesNotMatch(
      steps[index],
      /^\s+if:/m,
      `dependency gate cannot be skipped: ${command}`,
    );
    return index;
  });
  assert.ok(positions[0] < positions[1], "lock guard must precede npm ci");
  assert.ok(
    positions[1] < positions[2],
    "native import smoke must follow npm ci",
  );
  assert.ok(
    positions[1] < positions[3],
    "installed dependency tests must follow npm ci",
  );
}

test("Linux workflow rejects skipped, ignored or reordered dependency gates", async () => {
  const workflow = (
    await readFile(
      new URL("../.github/workflows/ci.yml", import.meta.url),
      "utf8",
    )
  ).replaceAll("\r\n", "\n");
  checkFrontendWorkflow(workflow);
  for (const mutate of [
    (source) =>
      source.replace(
        "node scripts/ci-frontend-dependencies.mjs --lock",
        "node --version",
      ),
    (source) =>
      source.replace(
        "node scripts/ci-frontend-dependencies.mjs --installed",
        "node --version",
      ),
    (source) =>
      source.replace(
        "node --test scripts/ci-frontend-dependencies.test.mjs",
        "node --version",
      ),
    (source) =>
      source.replace(
        "      - name: Verify Linux optional dependency lock before installation\n",
        "      - name: Verify Linux optional dependency lock before installation\n        if: github.event_name != 'pull_request'\n",
      ),
    (source) =>
      source.replace(
        "      - name: Verify installed native frontend dependencies\n",
        "      - name: Verify installed native frontend dependencies\n        continue-on-error: true\n",
      ),
    (source) =>
      source.replace(
        "  frontend:\n",
        "  frontend:\n    if: github.event_name != 'pull_request'\n",
      ),
    (source) =>
      source
        .replace(
          "node scripts/ci-frontend-dependencies.mjs --lock",
          "CI02_SWAP",
        )
        .replace("npm ci", "node scripts/ci-frontend-dependencies.mjs --lock")
        .replace("CI02_SWAP", "npm ci"),
    (source) =>
      source
        .replace(
          "node scripts/ci-frontend-dependencies.mjs --installed",
          "CI02_SWAP",
        )
        .replace(
          "npm ci",
          "node scripts/ci-frontend-dependencies.mjs --installed",
        )
        .replace("CI02_SWAP", "npm ci"),
  ]) {
    const changed = mutate(workflow);
    assert.notEqual(changed, workflow, "mutation must change workflow");
    assert.throws(() => checkFrontendWorkflow(changed));
  }
});

test("Linux Rollup native targets are complete and match the frozen public lock", () => {
  const result = checkFrontendDependencyLock(lock);
  assert.equal(
    result.rollup_version,
    lock.packages["node_modules/rollup"].version,
  );
  assert.ok(result.linux_native_targets >= 4);
});

test("Windows-generated lock regressions cannot silently remove Linux optional targets", () => {
  for (const mutate of [
    (input) => {
      delete input.packages[path];
    },
    (input) => {
      delete input.packages["node_modules/rollup"].optionalDependencies[target];
    },
    (input) => {
      input.packages["node_modules/rollup"].optionalDependencies[target] =
        "0.0.0";
    },
    (input) => {
      input.packages[path].version = "0.0.0";
    },
    (input) => {
      input.packages[path].optional = false;
    },
    (input) => {
      input.packages[path].os = ["win32"];
    },
    (input) => {
      input.packages[path].cpu = ["arm64"];
    },
    (input) => {
      input.packages[path].resolved = "https://npm.pkg.github.com/private.tgz";
    },
    (input) => {
      input.packages[path].integrity = "sha512-missing";
    },
  ]) {
    const input = copy();
    mutate(input);
    assert.throws(() => checkFrontendDependencyLock(input));
  }
});

test("all declared Linux targets are checked, including non-CI architectures", () => {
  const input = copy();
  const other = Object.keys(
    input.packages["node_modules/rollup"].optionalDependencies,
  ).find(
    (name) =>
      name.startsWith("@rollup/rollup-linux-") &&
      !name.includes("-x64-") &&
      !name.includes("-arm64-"),
  );
  assert.ok(other);
  delete input.packages[`node_modules/${other}`];
  assert.throws(
    () => checkFrontendDependencyLock(input),
    /missing Linux optional/,
  );
});

test("post-install smoke bundles a real module through the current host's native Rollup", async () => {
  const result = await checkInstalledRollup(lock);
  assert.equal(result.native_bundle_probe, true);
  assert.equal(result.platform, process.platform);
});

test("post-install smoke propagates missing optional/native binding failures", async () => {
  await assert.rejects(
    checkInstalledRollup(lock, async () => {
      throw new Error("Cannot find module @rollup/rollup-linux-x64-gnu");
    }),
    /Cannot find module/,
  );
  await assert.rejects(
    checkInstalledRollup(lock, async () => ({ VERSION: "0.0.0" })),
    /installed Rollup must match/,
  );
});

test("Rollup probe closes resources after generation errors", async () => {
  let closed = 0;
  await assert.rejects(
    checkInstalledRollup(lock, async () => ({
      VERSION: lock.packages["node_modules/rollup"].version,
      rollup: async () => ({
        generate: async () => {
          throw new Error("native parser failure");
        },
        close: async () => {
          closed += 1;
        },
      }),
    })),
    /native parser failure/,
  );
  assert.equal(closed, 1);
});
