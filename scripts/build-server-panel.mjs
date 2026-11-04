import { build } from "vite";
import vue from "@vitejs/plugin-vue";
import { createHash } from "node:crypto";
import { readFile, mkdir, writeFile, readdir } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, "apps/fono-server/resources/panel");
const check = process.argv.includes("--check");
const result = await build({
  root,
  configFile: false,
  plugins: [vue()],
  publicDir: false,
  define: { "process.env.NODE_ENV": '"production"' },
  build: {
    write: false,
    target: "es2022",
    sourcemap: false,
    minify: "esbuild",
    cssCodeSplit: false,
    outDir: output,
    lib: {
      entry: join(root, "src/v3/server.ts"),
      name: "FonoServerPanel",
      formats: ["iife"],
    },
    rollupOptions: {
      output: { entryFileNames: "panel.js", assetFileNames: "panel.[ext]" },
    },
  },
});
const files = new Map();
for (const bundle of Array.isArray(result) ? result : [result]) {
  for (const item of bundle.output)
    files.set(item.fileName, item.type === "chunk" ? item.code : item.source);
}
files.set(
  "index.html",
  `<!doctype html>
<html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Fono Server</title><link rel="stylesheet" href="/panel/panel.css"><script defer src="/panel/panel.js"></script></head><body><div id="app"></div><noscript>Для панели Fono Server нужен JavaScript. Статус доступен по /healthz.</noscript></body></html>
`,
);
const sourceFiles = [
  "src/v3/server.ts",
  "src/v3/styles/tokens.css",
  "src/v3/shared/presentation/AppIcon.vue",
  "src/v3/shared/presentation/SelectField.vue",
  "src/v3/shared/domain/icons.ts",
  "scripts/build-server-panel.mjs",
];
async function collect(directory) {
  for (const entry of await readdir(join(root, directory), {
    withFileTypes: true,
  })) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) await collect(path);
    else if (entry.isFile()) sourceFiles.push(path);
  }
}
await collect("src/v3/features/serverPanel");
const hash = createHash("sha256");
for (const path of sourceFiles.sort())
  hash
    .update(path)
    .update("\0")
    .update(
      (await readFile(join(root, path), "utf8")).replaceAll("\r\n", "\n"),
    );
const lock = JSON.parse(
  await readFile(join(root, "package-lock.json"), "utf8"),
);
delete lock.version;
delete lock.packages[""].version;
hash.update(JSON.stringify(lock));
const sha = (value) => createHash("sha256").update(value).digest("hex");
files.set(
  "manifest.json",
  JSON.stringify(
    {
      schema: 1,
      source_sha256: hash.digest("hex"),
      files: Object.fromEntries(
        [...files].map(([name, content]) => [name, sha(content)]),
      ),
    },
    null,
    2,
  ) + "\n",
);
if (!files.has("panel.js") || !files.has("panel.css") || files.size !== 4)
  throw new Error("Unexpected panel output");
if (!check) await mkdir(output, { recursive: true });
for (const [name, content] of files) {
  const path = join(output, name);
  if (check) {
    const existing = await readFile(path);
    if (sha(existing) !== sha(content))
      throw new Error(
        `${relative(root, path)} is stale; run npm run build:server-panel`,
      );
  } else await writeFile(path, content);
}
console.log(
  `Server panel ${check ? "verified" : "built"}: ${files.size} embedded files`,
);
