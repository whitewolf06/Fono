import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { readBounded, verifyInstaller } from "./release-support.mjs";
import { signerFixture } from "./release-test-fixtures.mjs";
test("Tauri signer compatibility and version-bound release manifest/config", async () => {
  const directory = await mkdtemp(join(tmpdir(), "fono-tauri-signature-test-"));
  try {
    const cli = fileURLToPath(
      new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url),
    );
    const run = (args, env = process.env) =>
      execFileSync(process.execPath, args, {
        env,
        stdio: "pipe",
        timeout: 30000,
      });
    const keyPath = join(directory, "test.key");
    const password = randomBytes(16).toString("hex");
    run([
      cli,
      "signer",
      "generate",
      "--ci",
      "--password",
      password,
      "--write-keys",
      keyPath,
    ]);
    const publicKey = (await readBounded(`${keyPath}.pub`, 4096))
      .toString()
      .trim();
    const { version } = JSON.parse(
      (
        await readBounded(
          fileURLToPath(new URL("../package.json", import.meta.url)),
          64000,
        )
      ).toString(),
    );
    const installer = join(directory, `Fono_${version}_x64-setup.exe`);
    await writeFile(
      installer,
      "Tauri signature compatibility fixture; never installed",
    );
    run([
      cli,
      "signer",
      "sign",
      "--private-key-path",
      keyPath,
      "--password",
      password,
      installer,
    ]);
    const env = {
      ...process.env,
      FONO_UPDATER_PUBLIC_KEY: publicKey,
      FONO_UPDATER_ENDPOINT: "https://example.com/latest.json",
    };
    const output = join(directory, "latest.json");
    const signature = (await readBounded(`${installer}.sig`, 8192))
      .toString()
      .trim();
    await verifyInstaller(installer, signature, publicKey);
    await assert.rejects(
      verifyInstaller(installer, signature, publicKey, version),
      /trusted version/,
    );
    // Generic `signer sign` deliberately carries no version; Tauri bundler does.
    // This versioned Ed25519 fixture exercises our manifest CLI's binding gate.
    const fixture = signerFixture(
      await readBounded(installer, 64000),
      "ED",
      version,
    );
    await writeFile(`${installer}.sig`, fixture.signature);
    const manifestEnv = { ...env, FONO_UPDATER_PUBLIC_KEY: fixture.publicKey };
    run(
      [
        fileURLToPath(new URL("./update-manifest.mjs", import.meta.url)),
        "--installer",
        installer,
        "--download-base",
        "https://example.com/releases/",
        "--output",
        output,
      ],
      manifestEnv,
    );
    const manifest = JSON.parse((await readBounded(output, 64000)).toString());
    assert.equal(manifest.version, version);
    assert.equal(
      manifest.platforms["windows-x86_64"].url,
      `https://example.com/releases/Fono_${version}_x64-setup.exe`,
    );
    run(
      [
        fileURLToPath(new URL("./update-release-config.mjs", import.meta.url)),
        "--output",
        join(directory, "update.conf.json"),
      ],
      env,
    );
    const config = JSON.parse(
      (
        await readBounded(join(directory, "update.conf.json"), 64000)
      ).toString(),
    );
    assert.equal(config.bundle.createUpdaterArtifacts, true);
    assert.equal(config.plugins.updater.pubkey, publicKey);
    assert.equal(config.plugins.updater.requireSignedVersion, true);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
