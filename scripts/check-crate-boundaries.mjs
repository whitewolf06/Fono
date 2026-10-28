import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// Internal edges currently used by the shared workspace. External crates are
// deliberately not allowlisted: the plan's list predates protocol/types and
// service/core dependencies that already exist in the working application.
export const sharedAllowed = Object.freeze({
  "fono-audio": [],
  "fono-core": [],
  "fono-service": [
    "fono-audio",
    "fono-core",
    "fono-stt",
    "fono-stt-protocol",
    "fono-types",
  ],
  "fono-stt": ["fono-audio", "fono-core", "fono-stt-protocol", "fono-types"],
  "fono-stt-protocol": [],
  "fono-stt-worker": ["fono-stt-protocol"],
  "fono-types": ["fono-stt-protocol"],
  "fono-wake": ["fono-audio"],
});

const sharedNames = new Set(Object.keys(sharedAllowed));

function relativeManifest(manifestPath, workspaceRoot) {
  return path.relative(workspaceRoot, manifestPath).replaceAll("\\", "/");
}

export function packageViolations(owner, dependencyNames, desktopName) {
  const allowed = new Set(
    owner === "fono-server" ? sharedNames : (sharedAllowed[owner] ?? []),
  );
  const violations = [];
  for (const name of new Set(dependencyNames)) {
    if (name === "tauri" || name.startsWith("tauri-")) {
      violations.push(`${owner} -> ${name}: Tauri is desktop-only`);
    } else if (name === desktopName || name === "fono-server") {
      violations.push(
        `${owner} -> ${name}: applications are not shared dependencies`,
      );
    } else if (owner === "fono-types" && name.startsWith("tokio")) {
      violations.push(
        `${owner} -> ${name}: shared types must not own a Tokio runtime`,
      );
    } else if (name.startsWith("fono-") && !allowed.has(name)) {
      violations.push(`${owner} -> ${name}: forbidden internal dependency`);
    }
  }
  return violations;
}

export function parseCargoTree(output, owner) {
  const lines = output.trim().split(/\r?\n/).filter(Boolean);
  if (lines.length === 0) throw new Error(`cargo tree for ${owner} is empty`);
  const packages = lines.map((line) => {
    const match =
      /^([a-zA-Z0-9][a-zA-Z0-9_-]*) v\d+\.\d+\.\d+(?:[-+][^\s]+)?(?:\s|$)/.exec(
        line,
      );
    if (!match)
      throw new Error(`Unrecognized cargo tree line for ${owner}: ${line}`);
    return match[1];
  });
  if (packages[0] !== owner) {
    throw new Error(
      `cargo tree root mismatch: expected ${owner}, got ${packages[0]}`,
    );
  }
  return packages.slice(1);
}

function manifestPaths(workspaceRoot) {
  const manifests = [];
  const crateDirectory = path.join(workspaceRoot, "crates");
  for (const entry of readdirSync(crateDirectory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    if (existsSync(path.join(crateDirectory, entry.name, "Cargo.toml"))) {
      manifests.push(`crates/${entry.name}/Cargo.toml`);
    }
  }
  if (existsSync(path.join(workspaceRoot, "apps/fono-server/Cargo.toml"))) {
    manifests.push("apps/fono-server/Cargo.toml");
  }
  return manifests;
}

export function inspectMetadata(
  metadata,
  workspaceRoot,
  existingManifests = manifestPaths(workspaceRoot),
) {
  const members = new Set(metadata.workspace_members);
  const packages = metadata.packages.filter((pkg) => members.has(pkg.id));
  const byManifest = new Map(
    packages.map((pkg) => [
      relativeManifest(pkg.manifest_path, workspaceRoot),
      pkg,
    ]),
  );
  const desktop = byManifest.get("src-tauri/Cargo.toml");
  const violations = [];
  if (!desktop)
    violations.push("src-tauri/Cargo.toml is not a workspace member");

  const shared = packages.filter((pkg) =>
    /^crates\/[^/]+\/Cargo\.toml$/.test(
      relativeManifest(pkg.manifest_path, workspaceRoot),
    ),
  );
  for (const pkg of shared) {
    if (!sharedNames.has(pkg.name)) {
      violations.push(`${pkg.name}: shared crate has no boundary policy`);
    }
    violations.push(
      ...packageViolations(
        pkg.name,
        pkg.dependencies.map((dependency) => dependency.name),
        desktop?.name,
      ),
    );
  }

  for (const manifest of existingManifests) {
    if (manifest.startsWith("crates/") && !byManifest.has(manifest)) {
      violations.push(`${manifest} is not a workspace member`);
    }
  }

  const serverManifest = "apps/fono-server/Cargo.toml";
  const server = byManifest.get(serverManifest);
  if (existingManifests.includes(serverManifest) && !server) {
    violations.push(`${serverManifest} is not a workspace member`);
  }
  if (server) {
    if (server.name !== "fono-server") {
      violations.push(`${serverManifest} must define package fono-server`);
    }
    violations.push(
      ...packageViolations(
        "fono-server",
        server.dependencies.map((dependency) => dependency.name),
        desktop?.name,
      ),
    );
  }

  return {
    desktopName: desktop?.name,
    targets: [
      ...shared.map((pkg) => pkg.name),
      ...(server ? ["fono-server"] : []),
    ],
    violations,
  };
}

function cargo(args) {
  return execFileSync("cargo", args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
}

export function checkWorkspace(
  metadata,
  workspaceRoot,
  readTree,
  existingManifests,
) {
  const inspected = inspectMetadata(metadata, workspaceRoot, existingManifests);
  const violations = [...inspected.violations];
  for (const target of inspected.targets) {
    const names = parseCargoTree(readTree(target), target);
    violations.push(...packageViolations(target, names, inspected.desktopName));
  }
  return { targets: inspected.targets, violations: [...new Set(violations)] };
}

async function main() {
  const metadata = JSON.parse(
    cargo(["metadata", "--no-deps", "--locked", "--format-version", "1"]),
  );
  const result = checkWorkspace(metadata, root, (name) =>
    cargo([
      "tree",
      "--locked",
      "--all-features",
      "--target",
      "all",
      "--edges",
      "normal,build,dev",
      "-p",
      name,
      "--prefix",
      "none",
      "--format",
      "{p}",
    ]),
  );
  if (result.violations.length > 0) {
    for (const violation of result.violations) console.error(violation);
    process.exitCode = 1;
    return;
  }
  console.log(`Crate boundaries OK: ${result.targets.join(", ")}`);
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  await main();
}
