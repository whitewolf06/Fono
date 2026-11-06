import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs";
import {
  mkdir,
  open,
  stat,
  copyFile,
  unlink,
  appendFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const fixtureManifest = JSON.parse(
  fs.readFileSync(
    path.join(root, "apps/fono-server/tests/fixtures/remote-client-cpu.json"),
    "utf8",
  ),
);

async function exists(file) {
  try {
    await stat(file);
    return true;
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

export async function verifyResource(file, specification) {
  const metadata = await stat(file);
  if (!metadata.isFile() || metadata.size !== specification.bytes)
    throw new Error(
      `Public CPU fixture size mismatch: ${specification.basename}`,
    );
  const hash = createHash("sha256");
  for await (const bytes of fs.createReadStream(file)) hash.update(bytes);
  if (hash.digest("hex") !== specification.sha256)
    throw new Error(
      `Public CPU fixture SHA-256 mismatch: ${specification.basename}`,
    );
  return path.resolve(file);
}

export async function downloadResource(file, specification, fetchResource) {
  await mkdir(path.dirname(file), { recursive: true });
  const temporary = `${file}.${randomUUID()}.partial`;
  const output = await open(temporary, "wx");
  let outputClosed = false;
  try {
    const response = await fetchResource(specification.url, {
      signal: AbortSignal.timeout(120_000),
      redirect: "follow",
    });
    if (!response.ok || !response.body)
      throw new Error(
        `Public CPU fixture download failed: ${specification.basename}`,
      );
    if (response.url && new URL(response.url).protocol !== "https:")
      throw new Error("Public CPU fixture download redirected away from HTTPS");
    const length = response.headers.get("content-length");
    if (length !== null && Number(length) !== specification.bytes)
      throw new Error(
        `Public CPU fixture download size mismatch: ${specification.basename}`,
      );
    const hash = createHash("sha256");
    let received = 0;
    for await (const bytes of response.body) {
      received += bytes.byteLength;
      if (received > specification.bytes)
        throw new Error(
          `Public CPU fixture download exceeded its bound: ${specification.basename}`,
        );
      hash.update(bytes);
      await output.writeFile(bytes);
    }
    if (
      received !== specification.bytes ||
      hash.digest("hex") !== specification.sha256
    )
      throw new Error(
        `Public CPU fixture download integrity failed: ${specification.basename}`,
      );
    await output.close();
    outputClosed = true;
    // An existing destination is never overwritten, including a corrupt cache.
    // Exclusive copy works on Windows sandboxes that forbid hard links. The
    // caller receives a path only after the complete destination is verified.
    await copyFile(temporary, file, fs.constants.COPYFILE_EXCL);
  } finally {
    if (!outputClosed) await output.close();
    await unlink(temporary);
  }
  return verifyResource(file, specification);
}

export async function prepareResources(options = {}, fetchResource = fetch) {
  if (fixtureManifest.schema !== 1)
    throw new Error("Unsupported public CPU fixture manifest");
  const directory = path.resolve(
    options.outDir ?? path.join(root, "build/remote-client-cpu"),
  );
  const resources = {};
  const candidates = {
    model: path.join(root, "build/stt-quality", fixtureManifest.model.basename),
    audio: path.join(
      root,
      "build/server-panel-linux-qa",
      fixtureManifest.audio.basename,
    ),
  };
  for (const key of ["model", "audio"]) {
    const specification = fixtureManifest[key];
    const explicit = key === "model" ? options.model : options.wav;
    const cached = candidates[key];
    const file = path.resolve(
      explicit ??
        ((await exists(cached))
          ? cached
          : path.join(directory, specification.basename)),
    );
    if (key === "model" && path.basename(file) !== specification.basename)
      throw new Error("The pinned tiny model must retain its managed basename");
    if (await exists(file)) {
      resources[key] = await verifyResource(file, specification);
    } else {
      if (explicit || !options.download)
        throw new Error(
          `Missing public CPU fixture ${specification.basename}; downloading requires explicit --download`,
        );
      resources[key] = await downloadResource(
        file,
        specification,
        fetchResource,
      );
    }
  }
  if (options.githubEnv) {
    if ([resources.model, resources.audio].some((file) => /[\r\n]/u.test(file)))
      throw new Error("Cannot export a fixture path containing a newline");
    await appendFile(
      options.githubEnv,
      `FONO_TST02_MODEL=${resources.model}\nFONO_TST02_WAV=${resources.audio}\n`,
      "utf8",
    );
  }
  return {
    schema: 1,
    model: resources.model,
    wav: resources.audio,
    model_sha256: fixtureManifest.model.sha256,
    audio_sha256: fixtureManifest.audio.sha256,
  };
}

function argumentsFor(values) {
  const options = {};
  const keys = new Map([
    ["--out-dir", "outDir"],
    ["--model", "model"],
    ["--wav", "wav"],
    ["--github-env", "githubEnv"],
  ]);
  for (let index = 0; index < values.length; index++) {
    if (values[index] === "--download") {
      if (options.download) throw new Error("Duplicate --download");
      options.download = true;
    } else {
      const key = keys.get(values[index]);
      const value = values[++index];
      if (!key || !value || value.startsWith("--") || options[key])
        throw new Error(
          "Expected --model, --wav, --out-dir, --github-env or explicit --download",
        );
      options[key] = value;
    }
  }
  return options;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  try {
    console.log(
      JSON.stringify(
        await prepareResources(argumentsFor(process.argv.slice(2))),
      ),
    );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
