import { watch } from "node:fs";
import { buildSite } from "./build.mjs";
import { createSiteServer, listen } from "./server.mjs";
import { output, root, source } from "./site.mjs";
import path from "node:path";

await buildSite();
const server = createSiteServer(output);
console.log(`Fono: ${await listen(server, 1430)}`);
console.log(
  "При изменении исходников сайт пересобирается. Обновите страницу в браузере.",
);

let pending;
let building = false;
let queued = false;
const rebuild = async () => {
  if (building) {
    queued = true;
    return;
  }
  building = true;
  try {
    await buildSite();
    console.log("Сайт обновлён.");
  } catch (error) {
    console.error(error.message);
  } finally {
    building = false;
    if (queued) {
      queued = false;
      await rebuild();
    }
  }
};
const schedule = () => {
  clearTimeout(pending);
  pending = setTimeout(rebuild, 120);
};
const watchers = [
  watch(source, { recursive: true }, schedule),
  watch(path.join(root, "site.config.mjs"), () => {
    console.log(
      "Конфигурация изменена: перезапустите dev-сервер для её применения.",
    );
  }),
];
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => {
    clearTimeout(pending);
    watchers.forEach((watcher) => watcher.close());
    server.close();
  });
}
