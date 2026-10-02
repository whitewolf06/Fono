import { copyFileSync, mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const icons = path.join(root, "src-tauri", "icons");
const cli = path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
const result = spawnSync(
  process.execPath,
  [cli, "icon", path.join(root, "app-icon.png"), "--output", icons],
  { cwd: root, stdio: "inherit" },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

const publicDir = path.join(root, "public");
mkdirSync(publicDir, { recursive: true });
for (const [source, destination] of [
  ["128x128.png", "fono-icon.png"],
  ["32x32.png", "favicon.png"],
  ["icon.ico", "favicon.ico"],
]) {
  copyFileSync(path.join(icons, source), path.join(publicDir, destination));
}
console.log("Fono icons synchronized: native bundles, tray, browser and UI.");
