import { signerFixture } from "./release-test-fixtures.mjs";
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  argumentsMap,
  assertSigningCliVersion,
  httpsUrl,
  MAX_MANIFEST_BYTES,
  parsePublicKey,
  parseSignature,
  readBounded,
  validateManifest,
  verifyInstaller,
} from "./release-support.mjs";

test("updater preparation requires a stable CLI with versioned bundle signatures", () => {
  for (const version of ["2.11.4", "2.10.99", "2.11.5-rc.1", "broken"]) {
    assert.throws(() => assertSigningCliVersion(version), /cli >=2\.11\.5/);
  }
  for (const version of ["2.11.5", "2.11.6", "2.12.0"]) {
    assert.equal(assertSigningCliVersion(version), version);
  }
});

async function withFile(callback) {
  const directory = await mkdtemp(join(tmpdir(), "fono-update-test-"));
  try {
    const path = join(directory, "test.exe");
    const bytes = Buffer.from("Fono installer fixture; never executed");
    await writeFile(path, bytes);
    await callback(path, bytes);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("accepts credential-free HTTPS and rejects token/HTTP URLs", () => {
  assert.equal(
    httpsUrl("https://example.com/releases/latest.json").protocol,
    "https:",
  );
  for (const value of [
    "http://example.com",
    "https://user:password@example.com",
    "https://example.com?token=secret",
    "https://example.com/#hash",
  ]) {
    assert.throws(() => httpsUrl(value));
  }
});

test("requires canonical bounded Tauri public key and signature content", () => {
  const fixture = signerFixture(Buffer.from("fixture"));
  assert.ok(parsePublicKey(fixture.publicKey));
  assert.equal(parseSignature(fixture.signature).algorithm, "ED");
  for (const invalid of ["publickey.pub", "YWJj!", "A".repeat(10000), ""]) {
    assert.throws(() => parsePublicKey(invalid));
    assert.throws(() => parseSignature(invalid));
  }
});

for (const algorithm of ["Ed", "ED"]) {
  test(`verifies real Ed25519 ${algorithm} Minisign payload and trusted comment`, async () => {
    await withFile(async (path, bytes) => {
      const fixture = signerFixture(bytes, algorithm);
      await verifyInstaller(path, fixture.signature, fixture.publicKey);
      await writeFile(path, Buffer.concat([bytes, Buffer.from("tampered")]));
      await assert.rejects(
        verifyInstaller(path, fixture.signature, fixture.publicKey),
        /signature does not verify/,
      );
    });
  });
}

test("rejects mismatched signing key before creating a channel manifest", async () => {
  await withFile(async (path, bytes) => {
    const first = signerFixture(bytes);
    const second = signerFixture(bytes);
    await assert.rejects(
      verifyInstaller(path, first.signature, second.publicKey),
      /key ID/,
    );
  });
});

test("rejects modified trusted comment even when the installer signature matches", async () => {
  await withFile(async (path, bytes) => {
    const fixture = signerFixture(bytes);
    const text = Buffer.from(fixture.signature, "base64")
      .toString()
      .replace("timestamp:1", "timestamp:2");
    const tampered = Buffer.from(text).toString("base64");
    await assert.rejects(
      verifyInstaller(path, tampered, fixture.publicKey),
      /trusted comment/,
    );
  });
});

test("validates version, supported platform, HTTPS artifact and UTC date", () => {
  const fixture = signerFixture(Buffer.from("fixture"));
  const manifest = {
    version: "0.5.21",
    notes: "",
    pub_date: "2026-10-04T00:00:00Z",
    platforms: {
      "windows-x86_64": {
        url: "https://example.com/Fono_0.5.21_x64-setup.exe",
        signature: fixture.signature,
      },
    },
  };
  assert.equal(validateManifest(manifest, "0.5.21"), manifest);
  assert.throws(() => validateManifest(manifest, "0.5.22"), /version/);
  assert.throws(
    () => validateManifest({ ...manifest, version: "0.5.22" }, "0.5.22"),
    /trusted version/,
  );
  assert.throws(
    () => validateManifest({ ...manifest, pub_date: "yesterday" }, "0.5.21"),
    /date/,
  );
  assert.throws(
    () =>
      validateManifest(
        { ...manifest, notes: "x".repeat(MAX_MANIFEST_BYTES) },
        "0.5.21",
      ),
    /notes/,
  );
  assert.throws(
    () => validateManifest({ ...manifest, platforms: {} }, "0.5.21"),
    /Windows/,
  );
});

test("file reads reject empty or oversized input", async () => {
  await withFile(async (path) => {
    await assert.rejects(readBounded(path, 2), /invalid size/);
    await writeFile(path, "");
    await assert.rejects(readBounded(path, 1024), /invalid size/);
  });
});

test("CLI parsing rejects missing values and duplicate options", () => {
  assert.deepEqual(argumentsMap(["--installer", "test.exe"]), {
    installer: "test.exe",
  });
  assert.throws(() => argumentsMap(["--installer"]));
  assert.throws(() =>
    argumentsMap(["--installer", "a.exe", "--installer", "b.exe"]),
  );
});

test("verifies the published Minisign prehashed test vector", async () => {
  await withFile(async (path) => {
    // Test vector from minisign-verify 0.2.5: message is exactly `test`.
    await writeFile(path, "test");
    const publicRecord =
      "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const signatureText = [
      "untrusted comment: signature from minisign secret key",
      "RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=",
      "trusted comment: timestamp:1633700835\tfile:test\tprehashed",
      "wLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==",
    ].join("\n");
    const publicKey = Buffer.from(
      `untrusted comment: public key\n${publicRecord}\n`,
    ).toString("base64");
    await verifyInstaller(
      path,
      Buffer.from(signatureText).toString("base64"),
      publicKey,
    );
  });
});
