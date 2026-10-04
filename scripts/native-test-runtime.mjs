import { createHash, randomUUID } from "node:crypto";
import {
  readFile,
  lstat,
  realpath,
  copyFile,
  rename,
  rm,
} from "node:fs/promises";
import {
  dirname,
  extname,
  isAbsolute,
  join,
  relative,
  resolve,
} from "node:path";
import { pathToFileURL } from "node:url";

export const RUNTIME_DLLS = [
  "onnxruntime.dll",
  "onnxruntime_providers_shared.dll",
  "sherpa-onnx-c-api.dll",
  "sherpa-onnx-cxx-api.dll",
];

function within(root, path) {
  const child = relative(root, path);
  return (
    child !== ".." &&
    !child.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) &&
    !isAbsolute(child)
  );
}

async function regularFile(path) {
  const info = await lstat(path);
  if (!info.isFile() || info.isSymbolicLink()) {
    throw new Error(
      `Expected a regular file, refusing linked runtime path: ${path}`,
    );
  }
  return path;
}

/** Cargo's emitted artifact paths remain authoritative for custom targets/profiles. */
export async function testExecutables(messages, targetDirectory) {
  const root = await realpath(targetDirectory);
  const executables = new Set();
  let finished = false;
  for (const line of messages.split(/\r?\n/).filter(Boolean)) {
    const message = JSON.parse(line);
    if (message.reason === "build-finished")
      finished = message.success === true;
    if (message.reason !== "compiler-artifact" || !message.executable) continue;
    const executable = message.executable;
    if (
      !isAbsolute(executable) ||
      extname(executable).toLowerCase() !== ".exe"
    ) {
      throw new Error(`Invalid native test executable: ${executable}`);
    }
    await regularFile(executable);
    const actual = await realpath(executable);
    if (
      !within(root, actual) ||
      !within(resolve(targetDirectory), resolve(executable))
    ) {
      throw new Error(
        `Native test executable is outside the selected Cargo target: ${executable}`,
      );
    }
    executables.add(actual);
  }
  if (!finished || executables.size === 0) {
    throw new Error(
      "Cargo did not finish successfully with native test executables.",
    );
  }
  return [...executables];
}

async function hash(path) {
  return createHash("sha256")
    .update(await readFile(path))
    .digest("hex");
}

/** Put the linked runtime ahead of System32; PATH alone cannot guarantee this. */
export async function stageTestRuntime(
  executables,
  runtimeDirectory,
  log = console.log,
) {
  const sources = await Promise.all(
    RUNTIME_DLLS.map(async (name) => {
      const path = await regularFile(join(runtimeDirectory, name));
      return { name, path, digest: await hash(path) };
    }),
  );
  const directories = [...new Set(executables.map(dirname))];
  // Validate every destination before replacing any stale DLL.
  for (const directory of directories) {
    for (const source of sources) {
      try {
        await regularFile(join(directory, source.name));
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
      }
    }
  }
  for (const directory of directories) {
    log(`Native test runtime: ${resolve(runtimeDirectory)} -> ${directory}`);
    for (const source of sources) {
      const destination = join(directory, source.name);
      let matches = false;
      try {
        matches = (await hash(destination)) === source.digest;
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
      }
      if (!matches) {
        const temporary = `${destination}.${randomUUID()}.part`;
        try {
          await copyFile(source.path, temporary);
          if ((await hash(temporary)) !== source.digest) {
            throw new Error(`Runtime copy checksum mismatch: ${source.name}`);
          }
          await rename(temporary, destination);
        } finally {
          await rm(temporary, { force: true });
        }
      }
      log(
        `  ${source.name}: SHA-256 ${source.digest}${matches ? " (already correct)" : " (staged)"}`,
      );
    }
  }
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  const [messagesPath, runtimeDirectory, targetDirectory] =
    process.argv.slice(2);
  if (
    !messagesPath ||
    !runtimeDirectory ||
    !targetDirectory ||
    process.argv.length !== 5
  ) {
    throw new Error(
      "Usage: native-test-runtime.mjs <cargo-json> <linked-runtime-directory> <target-directory>",
    );
  }
  const executables = await testExecutables(
    await readFile(messagesPath, "utf8"),
    targetDirectory,
  );
  await stageTestRuntime(executables, runtimeDirectory);
}
