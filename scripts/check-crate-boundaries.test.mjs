import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import {
  checkWorkspace,
  inspectMetadata,
  parseCargoTree,
} from "./check-crate-boundaries.mjs";

const fixtureRoot = path.resolve("crate-boundary-fixture");

function packageAt(name, manifest, dependencies = []) {
  return {
    id: name,
    name,
    manifest_path: path.join(fixtureRoot, manifest),
    dependencies,
  };
}

function fixture(sharedName = "fono-service", dependencies = []) {
  const manifest = `crates/${sharedName}/Cargo.toml`;
  const packages = [
    packageAt("fono", "src-tauri/Cargo.toml"),
    packageAt(sharedName, manifest, dependencies),
  ];
  return {
    metadata: {
      workspace_members: packages.map((pkg) => pkg.id),
      packages,
    },
    manifests: [manifest],
  };
}

function tree(owner, ...dependencies) {
  return [owner, ...dependencies].map((name) => `${name} v0.1.0`).join("\n");
}

test("the existing shared-service direction is accepted", () => {
  const { metadata, manifests } = fixture("fono-service", [
    { name: "fono-stt" },
    { name: "fono-core" },
    { name: "fono-types" },
  ]);
  const result = checkWorkspace(
    metadata,
    fixtureRoot,
    () => tree("fono-service", "fono-stt", "fono-core", "fono-types", "axum"),
    manifests,
  );
  assert.deepEqual(result.violations, []);
});

test("an optional Windows-only, renamed Tauri dependency fails before feature selection", () => {
  const { metadata, manifests } = fixture("fono-service", [
    {
      name: "tauri",
      rename: "desktop_api",
      optional: true,
      target: "cfg(windows)",
    },
  ]);
  const result = checkWorkspace(
    metadata,
    fixtureRoot,
    () => tree("fono-service", "fono-stt"),
    manifests,
  );
  assert.match(
    result.violations.join("\n"),
    /fono-service -> tauri: Tauri is desktop-only/,
  );
});

test("transitive Tauri plugins and build packages fail the tree check", () => {
  const { metadata, manifests } = fixture();
  for (const packageName of ["tauri-plugin-updater", "tauri-build"]) {
    const result = checkWorkspace(
      metadata,
      fixtureRoot,
      () => tree("fono-service", "axum", packageName),
      manifests,
    );
    assert.match(result.violations.join("\n"), /Tauri is desktop-only/);
  }
});

test("types cannot acquire Tokio and STT cannot depend on service", () => {
  const types = fixture("fono-types", [{ name: "tokio" }]);
  const typesResult = checkWorkspace(
    types.metadata,
    fixtureRoot,
    () => tree("fono-types", "tokio"),
    types.manifests,
  );
  assert.match(
    typesResult.violations.join("\n"),
    /shared types must not own a Tokio runtime/,
  );

  const stt = fixture("fono-stt", [{ name: "fono-service" }]);
  const sttResult = checkWorkspace(
    stt.metadata,
    fixtureRoot,
    () => tree("fono-stt", "fono-service"),
    stt.manifests,
  );
  assert.match(
    sttResult.violations.join("\n"),
    /forbidden internal dependency/,
  );
});

test("the future server cannot depend on desktop and must join the workspace", () => {
  const { metadata, manifests } = fixture();
  const serverManifest = "apps/fono-server/Cargo.toml";
  assert.match(
    inspectMetadata(metadata, fixtureRoot, [
      ...manifests,
      serverManifest,
    ]).violations.join("\n"),
    /apps\/fono-server\/Cargo\.toml is not a workspace member/,
  );

  const server = packageAt("fono-server", serverManifest, [{ name: "fono" }]);
  metadata.packages.push(server);
  metadata.workspace_members.push(server.id);
  const result = checkWorkspace(
    metadata,
    fixtureRoot,
    (owner) => tree(owner, ...(owner === "fono-server" ? ["fono"] : [])),
    [...manifests, serverManifest],
  );
  assert.match(
    result.violations.join("\n"),
    /fono-server -> fono: applications are not shared dependencies/,
  );
});

test("unrecognized or wrong-root cargo output fails closed", () => {
  assert.throws(() => parseCargoTree("", "fono-service"), /empty/);
  assert.throws(
    () =>
      parseCargoTree(
        "fono-service v0.1.0\nwarning: hidden dependency",
        "fono-service",
      ),
    /Unrecognized cargo tree line/,
  );
  assert.throws(
    () => parseCargoTree("fono-stt v0.1.0", "fono-service"),
    /root mismatch/,
  );
});
