import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  CUDA_FILES,
  CUDA_NOTICES,
  validateCudaManifest,
  verifyCudaArchive,
} from "./sign-cuda-package.mjs";
import { signerFixture } from "./release-test-fixtures.mjs";
import { verifyInstaller } from "./release-support.mjs";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
function manifest(bytes = Buffer.from("archive fixture never executed")) {
  return {
    schema_version: 1,
    purpose: "fono-cuda",
    app_version: "0.7.22",
    protocol_version: 3,
    platform: "windows-x86_64",
    revision: 1,
    archive: {
      name: "fono-cuda-0.7.22-windows-x86_64.tar.bz2",
      size: bytes.length,
      sha256: sha256(bytes),
    },
    files: [...CUDA_FILES, ...CUDA_NOTICES].map((path) => ({
      path,
      size: 1,
      sha256: "a".repeat(64),
    })),
    notices: [...CUDA_NOTICES],
  };
}

test("signed CUDA manifest binds exact purpose/platform/app/protocol/revision and file allowlist", () => {
  assert.ok(validateCudaManifest(manifest(), "0.7.22", 3));
  for (const [field, value] of [
    ["purpose", "fono-update"],
    ["schema_version", 2],
    ["app_version", "0.7.21"],
    ["protocol_version", 2],
    ["revision", 0],
    ["revision", Number.MAX_SAFE_INTEGER + 1],
    ["platform", "linux-x86_64"],
  ]) {
    assert.throws(() =>
      validateCudaManifest({ ...manifest(), [field]: value }, "0.7.22", 3),
    );
  }
  for (const mutate of [
    (item) =>
      item.files.push({ path: "extra.exe", size: 1, sha256: "a".repeat(64) }),
    (item) => (item.files[0].path = "../cublas64_13.dll"),
    (item) => (item.files[0].size = 0),
    (item) => (item.files[0].sha256 = "x".repeat(64)),
    (item) => (item.files[0].unexpected = true),
    (item) => item.notices.pop(),
    (item) => item.notices.reverse(),
    (item) => (item.archive.name = "https://other.example/file.tar.bz2"),
    (item) => (item.archive.size = 1024 ** 3 + 1),
    (item) => (item.url = "https://other.example/"),
  ]) {
    const item = manifest();
    mutate(item);
    assert.throws(() => validateCudaManifest(item, "0.7.22", 3));
  }
});

test("manifest exact bytes and trusted app version use the existing Minisign verifier", async () => {
  const directory = await mkdtemp(join(tmpdir(), "fono-cuda-signature-test-"));
  try {
    const path = join(directory, "manifest.json");
    const bytes = Buffer.from(JSON.stringify(manifest()));
    await writeFile(path, bytes);
    const fixture = signerFixture(bytes, "ED", "0.7.22");
    await verifyInstaller(path, fixture.signature, fixture.publicKey, "0.7.22");
    await assert.rejects(
      verifyInstaller(path, fixture.signature, fixture.publicKey, "0.7.23"),
      /trusted version/,
    );
    await writeFile(
      path,
      JSON.stringify({ ...manifest(), purpose: "other-purpose" }),
    );
    await assert.rejects(
      verifyInstaller(path, fixture.signature, fixture.publicKey, "0.7.22"),
      /does not verify/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("archive digest and size are mandatory before the release-only signing path", async () => {
  const directory = await mkdtemp(join(tmpdir(), "fono-cuda-hash-test-"));
  try {
    const path = join(directory, "archive.tar.bz2");
    const bytes = Buffer.from("archive fixture never executed");
    await writeFile(path, bytes);
    const item = manifest(bytes);
    await verifyCudaArchive(path, item);
    await writeFile(path, Buffer.alloc(bytes.length, "x"));
    await assert.rejects(verifyCudaArchive(path, item), /hash differs/);
    await writeFile(path, "short");
    await assert.rejects(verifyCudaArchive(path, item), /size differs/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("the shipped base config has one exact Vulkan resource and CPU override removes it", async () => {
  const base = JSON.parse(
    await readFile(
      new URL("../src-tauri/tauri.conf.json", import.meta.url),
      "utf8",
    ),
  );
  const cpu = JSON.parse(
    await readFile(
      new URL("../src-tauri/tauri.cpu.conf.json", import.meta.url),
      "utf8",
    ),
  );
  const workerResources = Object.fromEntries(
    Object.entries(base.bundle.resources).filter(([name]) =>
      name.includes("stt-"),
    ),
  );
  assert.deepEqual(workerResources, {
    "resources/stt-base/fono-stt-vulkan-worker.exe":
      "resources/stt-workers/fono-stt-vulkan-worker.exe",
  });
  assert.equal(
    cpu.bundle.resources["resources/stt-base/fono-stt-vulkan-worker.exe"],
    null,
  );
  assert.equal(base.bundle.resources["resources/sherpa-onnx/*.dll"], "");
  assert.equal(base.bundle.resources["resources/vad/*.onnx"], "resources/vad/");
});
