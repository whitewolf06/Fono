import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  downloadResource,
  fixtureManifest,
  prepareResources,
  verifyResource,
} from "./remote-client-cpu-resources.mjs";
import { modelSpec } from "./stt-quality.mjs";

test("CPU acceptance reuses the pinned public quality-model identity", () => {
  assert.equal(fixtureManifest.schema, 1);
  assert.equal(fixtureManifest.model.id, "whisper:tiny");
  assert.equal(fixtureManifest.model.basename, modelSpec.basename);
  assert.equal(fixtureManifest.model.sha256, modelSpec.sha256);
  assert.ok(fixtureManifest.model.url.includes(`/${modelSpec.revision}/`));
  assert.equal(fixtureManifest.model.bytes, 77_691_713);
  assert.equal(fixtureManifest.audio.samples, 176_000);
});

async function temporary(action) {
  const directory = await fs.mkdtemp(
    path.join(os.tmpdir(), "fono-public-cpu-test-"),
  );
  try {
    await action(directory);
  } finally {
    const target = await fs.realpath(directory);
    assert.equal(path.dirname(target), await fs.realpath(os.tmpdir()));
    assert.ok(path.basename(target).startsWith("fono-public-cpu-test-"));
    assert.equal((await fs.lstat(directory)).isSymbolicLink(), false);
    await fs.rm(directory, { recursive: true, force: true });
  }
}

const bytes = Buffer.from("public fixture test bytes");
const specification = {
  basename: "fixture.bin",
  bytes: bytes.length,
  sha256: createHash("sha256").update(bytes).digest("hex"),
  url: "https://example.invalid/pinned/fixture.bin",
};
const response = (body) => async () => new Response(body, { status: 200 });

test("verified download publishes complete bytes and removes partial files", async () => {
  await temporary(async (directory) => {
    const destination = path.join(directory, specification.basename);
    assert.equal(
      await downloadResource(destination, specification, response(bytes)),
      destination,
    );
    assert.deepEqual(await fs.readFile(destination), bytes);
    assert.deepEqual(await fs.readdir(directory), [specification.basename]);
  });
});

test("corrupt or oversized downloads cannot publish a fixture", async () => {
  await temporary(async (directory) => {
    const destination = path.join(directory, specification.basename);
    for (const body of [
      Buffer.alloc(bytes.length),
      Buffer.alloc(bytes.length + 1),
    ]) {
      await assert.rejects(
        downloadResource(destination, specification, response(body)),
      );
      assert.deepEqual(await fs.readdir(directory), []);
    }
  });
});

test("download does not overwrite an existing destination", async () => {
  await temporary(async (directory) => {
    const destination = path.join(directory, specification.basename);
    const existing = Buffer.from("preserve this existing artifact");
    await fs.writeFile(destination, existing);
    await assert.rejects(
      downloadResource(destination, specification, response(bytes)),
    );
    assert.deepEqual(await fs.readFile(destination), existing);
    assert.deepEqual(await fs.readdir(directory), [specification.basename]);
    await assert.rejects(verifyResource(destination, specification));
  });
});

test("missing explicitly supplied resource fails without network access", async () => {
  await temporary(async (directory) => {
    let requested = false;
    await assert.rejects(
      prepareResources(
        { model: path.join(directory, modelSpec.basename), download: true },
        async () => {
          requested = true;
          throw new Error(
            "network must not be used for an explicitly supplied cache",
          );
        },
      ),
      /Missing public CPU fixture/u,
    );
    assert.equal(requested, false);
    assert.deepEqual(await fs.readdir(directory), []);
  });
});

test("invalid existing public cache is preserved and rejected before fetch", async () => {
  await temporary(async (directory) => {
    const model = path.join(directory, modelSpec.basename);
    const existing = Buffer.from("invalid fixture, retain for diagnosis");
    await fs.writeFile(model, existing);
    let requested = false;
    await assert.rejects(
      prepareResources({ model, download: true }, async () => {
        requested = true;
        throw new Error(
          "a corrupt existing fixture must not trigger a download",
        );
      }),
      /Public CPU fixture size mismatch/u,
    );
    assert.equal(requested, false);
    assert.deepEqual(await fs.readFile(model), existing);
    assert.deepEqual(await fs.readdir(directory), [modelSpec.basename]);
  });
});
