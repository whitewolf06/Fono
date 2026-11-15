import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath, pathToFileURL } from "node:url";

export const DOCUMENTATION_GUIDES = Object.freeze([
  "README.md",
  "user-guide.md",
  "architecture.md",
  "code-rules.md",
  "frontend-architecture.md",
  "stt.md",
  "remote-recognizer.md",
  "api.md",
  "server.md",
  "development.md",
  "testing.md",
  "delivery.md",
]);

// These exact normative payloads retain links relative to their original
// directory. Changing the offline notice would also invalidate model packages.
// Only these two byte-pinned files use licenses/README.md for current navigation;
// every other Markdown file under licenses is checked normally.
const IMMUTABLE_NOTICES = new Map([
  [
    "licenses/offline-stt-NOTICE.md",
    "01b085bfd23e3bcd596ec1be35f891c461dc344d8ecc2859c5872329e03a7ad5",
  ],
  [
    "licenses/fono124-long-dictation-NOTICE.md",
    "4eaa62b768a7e3fa2c2add49e02b9a00ff629a903d4419c4ed9731651ca3a417",
  ],
]);

export function checkDocumentation(root) {
  const files = new Set();
  const failures = [];
  const relative = (file) =>
    path.relative(root, file).split(path.sep).join("/");
  const docs = path.join(root, "docs");
  const allowed = new Set(DOCUMENTATION_GUIDES);
  if (!fs.existsSync(docs) || !fs.lstatSync(docs).isDirectory()) {
    failures.push("Missing documentation directory: docs");
  } else {
    for (const entry of fs.readdirSync(docs, { withFileTypes: true })) {
      if (!entry.isFile() || !allowed.has(entry.name)) {
        failures.push(
          `Unexpected documentation entry docs/${entry.name}: only the twelve guide Markdown files belong in docs`,
        );
      }
    }
  }
  for (const name of DOCUMENTATION_GUIDES) {
    const file = path.join(docs, name);
    if (!fs.existsSync(file) || !fs.lstatSync(file).isFile()) {
      failures.push(`Missing documentation guide: docs/${name}`);
    } else {
      files.add(file);
    }
  }

  function collect(directory) {
    if (!fs.existsSync(directory)) return;
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name);
      if (entry.isSymbolicLink()) {
        failures.push(
          `Documentation source must not be a symlink: ${relative(full)}`,
        );
      } else if (entry.isDirectory()) {
        collect(full);
      } else if (entry.name.endsWith(".md")) {
        files.add(full);
      }
    }
  }
  for (const directory of [
    "tools/wake-bench",
    "tools/wake-training",
    "tools/wake-release",
    "src-tauri/tests/fixtures",
    "scripts/fixtures",
    "assets/readme",
    "licenses",
  ]) {
    collect(path.join(root, directory));
  }
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith(".md")) {
      files.add(path.join(root, entry.name));
    }
  }

  let checked = 0;
  let immutableNotices = 0;
  for (const file of files) {
    const bytes = fs.readFileSync(file);
    const identity = relative(file);
    const pinned = IMMUTABLE_NOTICES.get(identity);
    if (pinned) {
      if (createHash("sha256").update(bytes).digest("hex") !== pinned) {
        failures.push(`Immutable notice bytes changed: ${identity}`);
      }
      immutableNotices += 1;
      continue;
    }
    const source = bytes
      .toString("utf8")
      .replace(/^\s*(`{3,}|~{3,})[^\n]*\n[\s\S]*?^\s*\1\s*$/gm, "");
    for (const match of source.matchAll(
      /\[[^\]]*\]\((?:<([^>]+)>|([^\s)]+))(?:\s+"[^"]*")?\)/g,
    )) {
      const href = match[1] ?? match[2];
      if (/^(?:[a-z][a-z\d+.-]*:|#|\/\/)/i.test(href)) continue;
      let target;
      try {
        target = decodeURIComponent(href.split("#", 1)[0].split("?", 1)[0]);
      } catch {
        failures.push(`${identity}: invalid URI ${href}`);
        continue;
      }
      if (!target) continue;
      checked += 1;
      if (!fs.existsSync(path.resolve(path.dirname(file), target))) {
        failures.push(`${identity}: missing ${href}`);
      }
    }
  }
  return { failures, checked, files: files.size, immutableNotices };
}

function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const result = checkDocumentation(root);
  if (result.failures.length) {
    process.stderr.write(result.failures.join("\n") + "\n");
    process.exitCode = 1;
  } else {
    process.stdout.write(
      `Documentation: twelve guides; ${result.checked} local links across ${result.files} Markdown files; ${result.immutableNotices} byte-pinned notices. External URLs and heading anchors are not checked.\n`,
    );
  }
}

if (
  process.argv[1] &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
) {
  main();
}
