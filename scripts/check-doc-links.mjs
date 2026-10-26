import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const files = [];
function collect(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) collect(full);
    else if (entry.name.endsWith(".md")) files.push(full);
  }
}
collect(path.join(root, "docs"));
for (const directory of [
  "tools/wake-bench",
  "tools/wake-training",
  "tools/wake-release",
]) {
  collect(path.join(root, directory));
}
for (const name of [
  "README.md",
  "README.ru.md",
  "CONTRIBUTING.md",
  "SECURITY.md",
  "AGENTS.md",
  "THIRD_PARTY_NOTICES.md",
]) {
  const full = path.join(root, name);
  if (fs.existsSync(full)) files.push(full);
}
const failures = [];
let checked = 0;
for (const file of files) {
  const source = fs
    .readFileSync(file, "utf8")
    .replace(/^\s*(`{3,}|~{3,})[^\n]*\n[\s\S]*?^\s*\1\s*$/gm, "");
  for (const match of source.matchAll(
    /\[[^\]]*\]\((?:<([^>]+)>|([^\s)]+))(?:\s+"[^"]*")?\)/g,
  )) {
    const href = match[1] ?? match[2];
    if (/^(?:[a-z][a-z\d+.-]*:|#|\/\/)/i.test(href)) continue;
    let relative;
    try {
      relative = decodeURIComponent(href.split("#", 1)[0].split("?", 1)[0]);
    } catch {
      failures.push(`${path.relative(root, file)}: invalid URI ${href}`);
      continue;
    }
    if (!relative) continue;
    checked += 1;
    const target = path.resolve(path.dirname(file), relative);
    if (!fs.existsSync(target))
      failures.push(`${path.relative(root, file)}: missing ${href}`);
  }
}
if (failures.length) {
  process.stderr.write(failures.join("\n") + "\n");
  process.exitCode = 1;
} else {
  process.stdout.write(
    `Documentation links: ${checked} local targets across ${files.length} Markdown files exist. External URLs and heading anchors are not checked.\n`,
  );
}
