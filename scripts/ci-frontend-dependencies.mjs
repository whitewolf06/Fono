import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

// Runs before npm ci, so this guard must use only Node's built-in modules.
export function checkFrontendDependencyLock(lock) {
  assert.ok(lock.lockfileVersion >= 3, "npm lockfile v3 or newer is required");
  const packages = lock.packages;
  const rollup = packages?.["node_modules/rollup"];
  assert.ok(rollup?.version, "Rollup must have an exact locked version");
  const optional = rollup.optionalDependencies;
  assert.ok(optional, "Rollup native optional dependencies must be locked");
  const required = [
    "@rollup/rollup-linux-x64-gnu",
    "@rollup/rollup-linux-x64-musl",
    "@rollup/rollup-linux-arm64-gnu",
    "@rollup/rollup-linux-arm64-musl",
  ];
  for (const name of required) {
    assert.ok(
      optional[name],
      `Rollup is missing required Linux target: ${name}`,
    );
  }
  const linux = Object.keys(optional).filter((name) =>
    name.startsWith("@rollup/rollup-linux-"),
  );
  for (const name of linux) {
    const entry = packages[`node_modules/${name}`];
    assert.ok(entry, `missing Linux optional package lock entry: ${name}`);
    assert.equal(optional[name], rollup.version, `${name}: declared version`);
    assert.equal(entry.version, rollup.version, `${name}: locked version`);
    assert.equal(entry.optional, true, `${name}: must remain optional`);
    assert.deepEqual(entry.os, ["linux"], `${name}: Linux platform constraint`);
    const architecture = name
      .slice("@rollup/rollup-linux-".length)
      .split("-")[0];
    assert.deepEqual(entry.cpu, [architecture], `${name}: CPU constraint`);
    assert.equal(
      entry.resolved,
      `https://registry.npmjs.org/${name}/-/${name.slice("@rollup/".length)}-${rollup.version}.tgz`,
      `${name}: public immutable registry archive`,
    );
    assert.match(
      entry.integrity ?? "",
      /^sha512-[A-Za-z0-9+/]{86}==$/,
      `${name}: SHA-512 integrity`,
    );
    assert.equal(
      Buffer.from(entry.integrity.slice("sha512-".length), "base64").length,
      64,
      `${name}: complete SHA-512 digest`,
    );
  }
  return { rollup_version: rollup.version, linux_native_targets: linux.length };
}

export async function checkInstalledRollup(
  lock,
  load = () => import("rollup"),
) {
  const expected = checkFrontendDependencyLock(lock);
  const rollup = await load();
  assert.equal(
    rollup.VERSION,
    expected.rollup_version,
    "installed Rollup must match the lock",
  );
  // parseAst/native bindings are reached by a real bundle, rather than merely
  // checking that npm retained an unused optional package directory.
  const bundle = await rollup.rollup({
    input: "ci-frontend-probe",
    plugins: [
      {
        name: "ci-frontend-probe",
        resolveId: (id) => (id === "ci-frontend-probe" ? id : null),
        load: (id) =>
          id === "ci-frontend-probe" ? "export const answer = 6 * 7;" : null,
      },
    ],
  });
  try {
    const { output } = await bundle.generate({ format: "es" });
    assert.equal(output.length, 1, "Rollup probe must produce one module");
    assert.equal(output[0].type, "chunk");
    assert.match(output[0].code, /export\s*\{\s*answer\s*\}/);
    assert.match(output[0].code, /6 \* 7|42/);
  } finally {
    await bundle.close();
  }
  return {
    ...expected,
    native_bundle_probe: true,
    platform: process.platform,
    architecture: process.arch,
  };
}

async function main() {
  const args = process.argv.slice(2);
  assert.ok(
    args.length <= 1 && [undefined, "--lock", "--installed"].includes(args[0]),
    "usage: node scripts/ci-frontend-dependencies.mjs [--lock|--installed]",
  );
  const lock = JSON.parse(
    await readFile(new URL("../package-lock.json", import.meta.url), "utf8"),
  );
  const result =
    args[0] === "--installed"
      ? await checkInstalledRollup(lock)
      : checkFrontendDependencyLock(lock);
  console.log(JSON.stringify(result));
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
