import { createHash, createPublicKey, verify } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";

export const MAX_INSTALLER_BYTES = 512 * 1024 * 1024;
export const MAX_SIGNATURE_BYTES = 8192;
export const MAX_MANIFEST_BYTES = 64 * 1024;

export function assertSigningCliVersion(version) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(version);
  const parts = match?.slice(1).map(Number);
  const minimum = [2, 11, 5];
  if (parts?.every(Number.isSafeInteger)) {
    for (let index = 0; index < minimum.length; index += 1) {
      if (parts[index] > minimum[index]) return version;
      if (parts[index] < minimum[index]) break;
      if (index === minimum.length - 1) return version;
    }
  }
  throw new Error(
    "Updater preparation requires installed @tauri-apps/cli >=2.11.5 for versioned signatures; run npm ci with the current lockfile",
  );
}

export async function updateChannel(environment = process.env) {
  let publicKey = environment.FONO_UPDATER_PUBLIC_KEY?.trim();
  let endpoint = environment.FONO_UPDATER_ENDPOINT?.trim();
  if (!publicKey || !endpoint) {
    try {
      const channel = JSON.parse(
        (
          await readBounded(
            new URL("../src-tauri/update-channel.json", import.meta.url),
            MAX_MANIFEST_BYTES,
          )
        ).toString("utf8"),
      );
      publicKey ||= channel.publicKey;
      endpoint ||= channel.endpoint;
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
  parsePublicKey(publicKey);
  return { publicKey, endpoint: httpsUrl(endpoint, "Updater endpoint").href };
}

export function httpsUrl(value, label = "URL") {
  const url = new URL(value);
  if (
    url.protocol !== "https:" ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  ) {
    throw new Error(
      `${label} must be HTTPS without credentials, query or fragment`,
    );
  }
  return url;
}

function strictBase64(value, maximum, label) {
  if (
    typeof value !== "string" ||
    value.length > maximum ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(
      value,
    )
  ) {
    throw new Error(`Invalid ${label} base64`);
  }
  const bytes = Buffer.from(value, "base64");
  if (bytes.toString("base64") !== value) {
    throw new Error(`Noncanonical ${label} base64`);
  }
  return bytes;
}

// Tauri CLI wraps the complete Minisign text in one outer base64 string.
export function parsePublicKey(value) {
  const text = strictBase64(value?.trim(), 4096, "public key").toString("utf8");
  const lines = text.trim().split(/\r?\n/);
  if (lines.length !== 2 || !lines[0].startsWith("untrusted comment: ")) {
    throw new Error(
      "Expected a Tauri signer public key (file content, not its path)",
    );
  }
  const record = strictBase64(lines[1], 128, "public key record");
  if (
    record.length !== 42 ||
    !["Ed", "ED"].includes(record.subarray(0, 2).toString())
  ) {
    throw new Error("Unsupported Minisign public key");
  }
  return {
    id: record.subarray(2, 10),
    key: createPublicKey({
      key: Buffer.concat([
        Buffer.from("302a300506032b6570032100", "hex"),
        record.subarray(10),
      ]),
      format: "der",
      type: "spki",
    }),
  };
}

export function parseSignature(value) {
  const text = strictBase64(
    value?.trim(),
    MAX_SIGNATURE_BYTES,
    "signature",
  ).toString("utf8");
  const lines = text.trimEnd().split(/\r?\n/);
  if (
    lines.length !== 4 ||
    !lines[0].startsWith("untrusted comment: ") ||
    !lines[2].startsWith("trusted comment: ")
  ) {
    throw new Error("Invalid Tauri Minisign signature text");
  }
  const record = strictBase64(lines[1], 128, "signature record");
  const global = strictBase64(lines[3], 128, "global signature");
  const algorithm = record.subarray(0, 2).toString();
  if (
    record.length !== 74 ||
    global.length !== 64 ||
    !["Ed", "ED"].includes(algorithm)
  ) {
    throw new Error("Unsupported Minisign signature");
  }
  return {
    algorithm,
    id: record.subarray(2, 10),
    bytes: record.subarray(10),
    global,
    comment: lines[2].slice("trusted comment: ".length),
  };
}

export async function readBounded(path, maximum) {
  const metadata = await stat(path);
  if (!metadata.isFile() || metadata.size === 0 || metadata.size > maximum) {
    throw new Error(`File has invalid size (limit ${maximum} bytes): ${path}`);
  }
  const data = await readFile(path);
  if (data.length > maximum)
    throw new Error(`File exceeded size limit: ${path}`);
  return data;
}

export async function fileDigest(path, algorithm = "sha256") {
  const hash = createHash(algorithm);
  let count = 0;
  for await (const chunk of createReadStream(path)) {
    count += chunk.length;
    if (count > MAX_INSTALLER_BYTES)
      throw new Error("Installer exceeds 512 MiB limit");
    hash.update(chunk);
  }
  if (count === 0) throw new Error("Installer is empty");
  return hash.digest();
}

export function verifySignedVersion(signature, expectedVersion) {
  const versions = signature.comment
    .split("\t")
    .filter((field) => field.startsWith("version:"))
    .map((field) => field.slice("version:".length));
  if (versions.length !== 1 || versions[0] !== expectedVersion) {
    throw new Error(
      "Signature must contain exactly one trusted version matching package.json",
    );
  }
}

export async function verifyInstaller(
  path,
  signatureValue,
  publicKeyValue,
  expectedVersion,
) {
  const publicKey = parsePublicKey(publicKeyValue);
  const signature = parseSignature(signatureValue);
  if (!publicKey.id.equals(signature.id))
    throw new Error("Signature key ID differs from configured public key");
  const message =
    signature.algorithm === "ED"
      ? await fileDigest(path, "blake2b512")
      : await readBounded(path, MAX_INSTALLER_BYTES);
  if (!verify(null, message, publicKey.key, signature.bytes)) {
    throw new Error("Installer signature does not verify");
  }
  const globalMessage = Buffer.concat([
    signature.bytes,
    Buffer.from(signature.comment),
  ]);
  if (!verify(null, globalMessage, publicKey.key, signature.global)) {
    throw new Error("Signature trusted comment does not verify");
  }
  if (expectedVersion !== undefined)
    verifySignedVersion(signature, expectedVersion);
}

export function validateManifest(manifest, expectedVersion) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest))
    throw new Error("Expected manifest object");
  if (
    !/^\d+\.\d+\.\d+$/.test(manifest.version) ||
    manifest.version !== expectedVersion
  ) {
    throw new Error("Manifest version differs from package.json");
  }
  if (typeof manifest.notes !== "string" || manifest.notes.length > 16000)
    throw new Error("Invalid release notes");
  if (
    typeof manifest.pub_date !== "string" ||
    !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{3})?Z$/.test(manifest.pub_date) ||
    !Number.isFinite(Date.parse(manifest.pub_date))
  )
    throw new Error("Manifest date must be UTC RFC3339");
  const platforms = manifest.platforms;
  if (!platforms || Object.keys(platforms).join() !== "windows-x86_64")
    throw new Error("Only Windows x64 is supported by this release pipeline");
  const platform = platforms["windows-x86_64"];
  const url = httpsUrl(platform.url, "Installer URL");
  if (!url.pathname.endsWith(".exe"))
    throw new Error("Updater artifact must be the signed NSIS installer");
  verifySignedVersion(parseSignature(platform.signature), expectedVersion);
  if (Buffer.byteLength(JSON.stringify(manifest)) > MAX_MANIFEST_BYTES)
    throw new Error("Manifest exceeds 64 KiB");
  return manifest;
}

export function argumentsMap(argv) {
  const result = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    if (!name.startsWith("--") || !argv[index + 1] || result[name.slice(2)]) {
      throw new Error("Arguments must be unique --name value pairs");
    }
    result[name.slice(2)] = argv[index + 1];
  }
  return result;
}
