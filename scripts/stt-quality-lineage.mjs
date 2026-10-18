import { createHash, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
export function qualityLineage() {
  const inputs = [
    "src-tauri/crates/fono-audio",
    "src-tauri/crates/fono-stt-protocol",
    "src-tauri/crates/fono-stt-worker",
    "src-tauri/src/application/speech_gate.rs",
    "src-tauri/src/application/dictation/capture.rs",
    "src-tauri/src/vad",
    "src-tauri/Cargo.lock",
    "src-tauri/Cargo.toml",
    "src-tauri/rust-toolchain.toml",
    "src-tauri/.cargo/config.toml",
    "scripts/stt-quality.mjs",
    "scripts/stage4-quality.mjs",
    "scripts/stage5-quality.mjs",
    "scripts/stt-quality-lineage.mjs",
    "scripts/prepare-stt-quality-corpus.py",
    "scripts/bootstrap-ci-resources.ps1",
    "scripts/native-test-runtime.mjs",
    ".github/workflows/stt-quality.yml",
    "src-tauri/tests/fixtures/stt-quality/manifest.json",
    "src-tauri/tests/fixtures/stt-quality/baseline.json",
  ];
  const files = [];
  function visit(relative) {
    const absolute = path.join(root, relative);
    if (fs.statSync(absolute).isDirectory()) {
      for (const name of fs.readdirSync(absolute).sort())
        visit(`${relative}/${name}`);
    } else if (/\.(?:rs|toml|lock|mjs|json|py|ps1|yml)$/.test(relative)) {
      files.push({ path: relative, sha256: digest(fs.readFileSync(absolute)) });
    }
  }
  for (const input of inputs) visit(input);
  files.sort((a, b) => a.path.localeCompare(b.path, "en"));
  const revision = spawnSync("git", ["rev-parse", "HEAD"], {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
  });
  if (revision.status !== 0 || !/^[a-f0-9]{40}$/.test(revision.stdout.trim()))
    throw new Error("Cannot identify quality source revision");
  return {
    run_id: process.env.STT_QUALITY_RUN_ID || randomUUID(),
    source_revision: revision.stdout.trim(),
    app_version: JSON.parse(fs.readFileSync(path.join(root, "package.json")))
      .version,
    source_inputs_sha256: digest(Buffer.from(JSON.stringify(files))),
    source_inputs: files,
  };
}
