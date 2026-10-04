import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
import { escapeHtml } from "../src/templates/html.mjs";
import {
  downloadPanel,
  getConfig,
  linksFor,
  metadataFor,
  output,
  pages,
  source,
} from "./site.mjs";

export async function buildSite({
  destination = output,
  config = getConfig(),
} = {}) {
  const layout = path.join(source, "templates", "layout.mjs");
  const fingerprint = createHash("sha256")
    .update(await readFile(layout))
    .digest("hex");
  const { renderLayout } = await import(
    `${pathToFileURL(layout).href}?v=${fingerprint}`
  );
  const partialFiles = {
    dictationInfographic: "dictation-infographic.html",
    capabilitiesTable: "capabilities-table.html",
    systemRequirements: "system-requirements.html",
  };
  const partials = Object.fromEntries(
    await Promise.all(
      Object.entries(partialFiles).map(async ([name, file]) => [
        name,
        await readFile(path.join(source, "partials", file), "utf8"),
      ]),
    ),
  );
  await mkdir(destination, { recursive: true });
  // Only generated files owned by this builder are removed.
  for (const name of [
    "assets",
    ...pages.filter((page) => page.route).map((page) => page.route),
  ]) {
    await rm(path.join(destination, name), { recursive: true, force: true });
  }
  await cp(path.join(source, "assets"), path.join(destination, "assets"), {
    recursive: true,
  });
  await cp(
    path.join(source, "styles"),
    path.join(destination, "assets", "css"),
    { recursive: true },
  );
  await cp(path.join(source, "js"), path.join(destination, "assets", "js"), {
    recursive: true,
  });
  for (const page of pages) {
    const { links, assets } = linksFor(page);
    const replacements = {
      assets,
      homeUrl: links.home,
      featuresUrl: links.features,
      downloadPageUrl: links.download,
      helpUrl: links.help,
      privacyUrl: links.privacy,
      downloadPanel: downloadPanel(config),
    };
    const raw = await readFile(
      path.join(source, "pages", `${page.id}.html`),
      "utf8",
    );
    const expanded = raw.replace(
      /\{\{(\w+)\}\}/g,
      (token, key) => partials[key] ?? token,
    );
    const content = expanded.replace(/\{\{(\w+)\}\}/g, (_, key) => {
      if (!(key in replacements))
        throw new Error(`Неизвестный HTML placeholder: ${key}`);
      return key === "downloadPanel"
        ? replacements[key]
        : escapeHtml(replacements[key]);
    });
    const directory = path.join(destination, page.route);
    await mkdir(directory, { recursive: true });
    await writeFile(
      path.join(directory, "index.html"),
      renderLayout({
        page,
        content,
        assets,
        links,
        metadata: metadataFor(page, assets, config),
      }),
    );
  }
  const errorPage = {
    id: "not-found",
    title: "Страница не найдена — Fono",
    description: "Вернитесь на главную страницу Fono.",
  };
  const base = config.siteUrl ? new URL(config.siteUrl).pathname : "/";
  await writeFile(
    path.join(destination, "404.html"),
    renderLayout({
      page: errorPage,
      content: `<section class="page-hero"><div class="container"><p class="eyebrow">Ошибка 404</p><h1>Здесь пока тихо.</h1><p class="lead">Такой страницы нет. Вернитесь на главную или откройте помощь.</p><a class="button button-primary" href="${escapeHtml(base)}">На главную</a></div></section>`,
      assets: `${base}assets/`,
      links: Object.fromEntries(
        pages.map((page) => [
          page.id,
          `${base}${page.route ? `${page.route}/` : ""}`,
        ]),
      ),
      metadata: '<meta name="robots" content="noindex, nofollow">',
    }),
  );
  const sitemapPath = path.join(destination, "sitemap.xml");
  if (config.siteUrl) {
    const entries = pages.map(
      (page) =>
        `<url><loc>${escapeHtml(new URL(page.route ? `${page.route}/` : "./", config.siteUrl).href)}</loc></url>`,
    );
    await writeFile(
      sitemapPath,
      `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${entries.join("\n")}</urlset>\n`,
    );
  } else {
    await rm(sitemapPath, { force: true });
  }
  await writeFile(
    path.join(destination, "robots.txt"),
    config.siteUrl
      ? `User-agent: *\nAllow: /\nSitemap: ${new URL("sitemap.xml", config.siteUrl).href}\n`
      : "User-agent: *\nDisallow: /\n",
  );
  return destination;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  await buildSite();
  console.log(`Fono: 5 страниц собраны в ${output}`);
}
