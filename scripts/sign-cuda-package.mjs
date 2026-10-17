import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import {
  argumentsMap,
  assertSigningCliVersion,
  readBounded,
  updateChannel,
  verifyInstaller,
} from "./release-support.mjs";

export const CUDA_FILES = [
  "cublas64_13.dll",
  "cublasLt64_13.dll",
  "cudart64_13.dll",
  "fono-stt-cuda-worker.exe",
];
export const CUDA_NOTICES = [
  "THIRD-PARTY-NOTICES.txt",
  "NVIDIA-CUDA-EULA.txt",
  "NVIDIA-CUDA-third-party.txt",
  "FONO-LICENSE.txt",
  "WHISPER-CPP-LICENSE.txt",
  "DEPENDENCY-NOTICES.txt",
];

function exactKeys(value, keys, label) {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).sort().join() !== [...keys].sort().join()
  )
    throw new Error(`Invalid ${label} fields`);
}

export function validateCudaManifest(manifest, version, protocol) {
  exactKeys(
    manifest,
    [
      "schema_version",
      "purpose",
      "app_version",
      "protocol_version",
      "platform",
      "revision",
      "archive",
      "files",
      "notices",
    ],
    "CUDA manifest",
  );
  if (
    manifest.schema_version !== 1 ||
    manifest.purpose !== "fono-cuda" ||
    manifest.platform !== "windows-x86_64" ||
    !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version) ||
    manifest.app_version !== version ||
    manifest.protocol_version !== protocol ||
    !Number.isSafeInteger(protocol) ||
    protocol < 1 ||
    protocol > 65535 ||
    !Number.isSafeInteger(manifest.revision) ||
    manifest.revision < 1
  )
    throw new Error(
      "CUDA manifest purpose/version/protocol/platform/revision differs",
    );
  exactKeys(manifest.archive, ["name", "size", "sha256"], "archive");
  if (manifest.archive.name !== `fono-cuda-${version}-windows-x86_64.tar.bz2`)
    throw new Error("Invalid fixed CUDA archive asset name");
  const digest = (entry, maximum) => {
    if (
      !Number.isSafeInteger(entry.size) ||
      entry.size < 1 ||
      entry.size > maximum ||
      !/^[a-f0-9]{64}$/.test(entry.sha256)
    )
      throw new Error("Invalid signed hash/size");
  };
  digest(manifest.archive, 1024 ** 3);
  const required = [...CUDA_FILES, ...CUDA_NOTICES].sort();
  if (
    !Array.isArray(manifest.files) ||
    manifest.files
      .map((file) => file.path)
      .sort()
      .join() !== required.join()
  )
    throw new Error("Unexpected CUDA file allowlist");
  let total = 0;
  for (const file of manifest.files) {
    exactKeys(file, ["path", "size", "sha256"], "file");
    digest(file, 1024 ** 3);
    total += file.size;
  }
  if (total > 2 * 1024 ** 3)
    throw new Error("CUDA unpacked size exceeds limit");
  if (
    !Array.isArray(manifest.notices) ||
    manifest.notices.join() !== CUDA_NOTICES.join()
  )
    throw new Error("Missing CUDA license notices");
  return manifest;
}

export async function verifyCudaArchive(path, manifest) {
  const metadata = await stat(path);
  if (!metadata.isFile() || metadata.size !== manifest.archive.size)
    throw new Error("CUDA archive size differs");
  const hash = createHash("sha256");
  let size = 0;
  for await (const chunk of createReadStream(path)) {
    size += chunk.length;
    if (size > manifest.archive.size)
      throw new Error("CUDA archive grew during verification");
    hash.update(chunk);
  }
  if (
    size !== manifest.archive.size ||
    hash.digest("hex") !== manifest.archive.sha256
  )
    throw new Error("CUDA archive hash differs");
}

async function main() {
  const options = argumentsMap(process.argv.slice(2));
  if (
    !options.manifest ||
    !options["private-key-path"] ||
    Object.keys(options).some(
      (name) => !["manifest", "private-key-path"].includes(name),
    )
  )
    throw new Error(
      "Usage: node scripts/sign-cuda-package.mjs --manifest <prepared.json> --private-key-path <explicit external key file>",
    );
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const version = JSON.parse(
    await readFile(resolve(repo, "package.json"), "utf8"),
  ).version;
  const protocolSource = await readFile(
    resolve(repo, "src-tauri/crates/fono-stt-protocol/src/lib.rs"),
    "utf8",
  );
  const protocol = Number(
    protocolSource.match(/pub const PROTOCOL_VERSION:\s*u16\s*=\s*(\d+)/)?.[1],
  );
  const path = resolve(options.manifest);
  const bytes = await readBounded(path, 64 * 1024);
  const manifest = validateCudaManifest(JSON.parse(bytes), version, protocol);
  await verifyCudaArchive(
    resolve(dirname(path), manifest.archive.name),
    manifest,
  );
  const inspected = spawnSync(
    "python",
    [resolve(repo, "scripts/cuda-package.py"), "verify", "--manifest", path],
    { cwd: repo, encoding: "utf8", windowsHide: true },
  );
  if (inspected.error || inspected.status !== 0)
    throw new Error("CUDA archive content inspection failed before signing");
  const cli = JSON.parse(
    await readFile(
      resolve(repo, "node_modules/@tauri-apps/cli/package.json"),
      "utf8",
    ),
  );
  assertSigningCliVersion(cli.version);
  // This explicit release-only invocation is the sole place where the official
  // CLI is allowed to open an operator-selected private key. No auto discovery,
  // no key-content argument, no publication, and no secret text in tool output.
  const result = spawnSync(
    process.execPath,
    [
      resolve(repo, "node_modules/@tauri-apps/cli/tauri.js"),
      "signer",
      "sign",
      "--private-key-path",
      resolve(options["private-key-path"]),
      "--app-version",
      version,
      path,
    ],
    {
      cwd: repo,
      env: {
        ...process.env,
        TAURI_SIGNING_PRIVATE_KEY: "",
        TAURI_SIGNING_PRIVATE_KEY_PATH: "",
      },
      encoding: "utf8",
      windowsHide: true,
    },
  );
  if (result.error || result.status !== 0)
    throw new Error("Official Tauri signing failed; no release was published");
  const { publicKey } = await updateChannel({});
  const signature = (await readBounded(`${path}.sig`, 8192))
    .toString("utf8")
    .trim();
  await verifyInstaller(path, signature, publicKey, version);
  console.log(
    "CUDA manifest signed and verified against the embedded public key; no publication performed.",
  );
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
