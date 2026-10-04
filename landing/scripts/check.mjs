import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { once } from "node:events";
import { buildSite } from "./build.mjs";
import { createSiteServer, listen } from "./server.mjs";
import { getConfig, pages, root } from "./site.mjs";

const temporary = await mkdtemp(path.join(tmpdir(), "fono-site-check-"));
let server;

async function checkLinks(file, html, directory) {
  for (const [, reference] of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
    if (/^(?:https?:|mailto:|tel:|data:|#)/.test(reference)) continue;
    const clean = reference.split(/[?#]/)[0];
    let target = path.resolve(path.dirname(file), clean);
    if ((await stat(target)).isDirectory())
      target = path.join(target, "index.html");
    assert.ok(
      target.startsWith(`${directory}${path.sep}`),
      `Ссылка вне сборки: ${reference}`,
    );
    await stat(target);
    const anchor = reference.split("#")[1];
    if (anchor && path.extname(target) === ".html") {
      const linkedPage = await readFile(target, "utf8");
      assert.ok(
        linkedPage.includes(`id="${anchor}"`),
        `Нет якоря: ${reference}`,
      );
    }
  }
}

async function checkPages(directory, indexed) {
  const titles = new Set();
  const descriptions = new Set();
  for (const page of pages) {
    const file = path.join(directory, page.route, "index.html");
    const html = await readFile(file, "utf8");
    assert.match(html, /<html[^>]*lang="ru"/);
    assert.equal(
      (html.match(/<h1(?:\s|>)/g) ?? []).length,
      1,
      `${page.id}: один H1`,
    );
    assert.doesNotMatch(
      html,
      /\{\{|\}\}/,
      `${page.id}: необработанные placeholders`,
    );
    assert.match(html, /<meta property="og:image"/);
    const title = html.match(/<title>([^<]+)<\/title>/)?.[1];
    const description = html.match(
      /<meta\s+name="description"\s+content="([^"]+)"/,
    )?.[1];
    assert.ok(title && description, `${page.id}: SEO metadata`);
    assert.ok(
      !titles.has(title) && !descriptions.has(description),
      `${page.id}: уникальные metadata`,
    );
    titles.add(title);
    descriptions.add(description);
    assert.match(
      html,
      indexed ? /content="index, follow"/ : /content="noindex, nofollow"/,
    );
    if (indexed) assert.match(html, /rel="canonical"/);
    else assert.doesNotMatch(html, /rel="canonical"/);
    await checkLinks(file, html, directory);
  }
  const cssDirectory = path.join(directory, "assets", "css");
  for (const name of await readdir(cssDirectory)) {
    const css = await readFile(path.join(cssDirectory, name), "utf8");
    for (const [, reference] of css.matchAll(/url\(["']?([^"')]+)["']?\)/g)) {
      if (reference.startsWith("data:")) continue;
      await stat(path.resolve(cssDirectory, reference));
    }
  }
}

try {
  const manifest = JSON.parse(
    await readFile(path.join(root, "package.json"), "utf8"),
  );
  assert.ok(
    !manifest.dependencies && !manifest.devDependencies,
    "Сайт не требует npm-пакетов",
  );
  const preview = path.join(temporary, "preview");
  await buildSite({
    destination: preview,
    config: getConfig({ siteUrl: "", downloadUrl: "" }),
  });
  await checkPages(preview, false);
  assert.match(
    await readFile(path.join(preview, "download", "index.html"), "utf8"),
    /Бесплатный установщик готовится к публикации/,
  );
  assert.match(
    await readFile(path.join(preview, "robots.txt"), "utf8"),
    /Disallow: \//,
  );
  await assert.rejects(stat(path.join(preview, "sitemap.xml")), {
    code: "ENOENT",
  });

  const publicSite = path.join(temporary, "public");
  const config = getConfig({
    siteUrl: "https://fono.example/site",
    downloadUrl:
      "https://downloads.example/fono.exe?channel=stable&source=site",
  });
  await buildSite({ destination: publicSite, config });
  await checkPages(publicSite, true);
  const sitemap = await readFile(path.join(publicSite, "sitemap.xml"), "utf8");
  assert.equal((sitemap.match(/<loc>/g) ?? []).length, 5);
  for (const page of pages)
    assert.ok(
      sitemap.includes(
        new URL(page.route ? `${page.route}/` : "./", config.siteUrl).href,
      ),
    );
  const downloadHtml = await readFile(
    path.join(publicSite, "download", "index.html"),
    "utf8",
  );
  assert.match(downloadHtml, /Скачать бесплатно для Windows/);
  assert.match(downloadHtml, /channel=stable&amp;source=site/);
  assert.doesNotMatch(downloadHtml, /установщик готовится/);
  const errorHtml = await readFile(path.join(publicSite, "404.html"), "utf8");
  assert.match(errorHtml, /href="\/site\/"/);
  assert.match(errorHtml, /href="\/site\/assets\/css\/tokens.css"/);
  assert.match(errorHtml, /content="noindex, nofollow"/);
  // Rebuilding a former public output as a preview must remove the sitemap.
  await buildSite({
    destination: publicSite,
    config: getConfig({ siteUrl: "", downloadUrl: "" }),
  });
  await assert.rejects(stat(path.join(publicSite, "sitemap.xml")), {
    code: "ENOENT",
  });

  server = createSiteServer(preview);
  const address = await listen(server);
  for (const page of pages) {
    const response = await fetch(
      `${address}/${page.route ? `${page.route}/` : ""}`,
    );
    assert.equal(response.status, 200);
    assert.match(response.headers.get("content-type"), /text\/html/);
  }
  assert.equal(
    (await fetch(`${address}/features`, { redirect: "manual" })).status,
    301,
  );
  assert.equal((await fetch(`${address}/missing/`)).status, 404);
  assert.equal(
    (await fetch(`${address}/missing/`, { method: "HEAD" })).status,
    404,
  );
  assert.equal((await fetch(`${address}/`, { method: "POST" })).status, 405);
  assert.equal(
    (await fetch(`${address}/assets/js/navigation.js`)).headers.get(
      "content-type",
    ),
    "text/javascript; charset=utf-8",
  );
  assert.equal((await fetch(`${address}/%2e%2e%5cpackage.json`)).status, 400);
  console.log(
    "Fono website: 5 страниц, ссылки, SEO, preview/public, скачивание и HTTP — проверены.",
  );
} finally {
  if (server?.listening) {
    const closed = once(server, "close");
    server.close();
    server.closeAllConnections();
    await closed;
  }
  await rm(temporary, { recursive: true, force: true });
}
