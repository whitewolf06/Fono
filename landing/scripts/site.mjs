import path from "node:path";
import { fileURLToPath } from "node:url";
import settings from "../site.config.mjs";
import { escapeHtml } from "../src/templates/html.mjs";

export const root = fileURLToPath(new URL("../", import.meta.url));
export const source = path.join(root, "src");
export const output = path.join(root, "dist");
export const pages = [
  {
    id: "home",
    route: "",
    title: "Fono — голосовой ввод для Windows",
    description:
      "Говорите вместо печати: локальное распознавание, диктовка в активное окно и обработка текста. Бесплатное приложение Fono для Windows.",
  },
  {
    id: "features",
    route: "features",
    title: "Возможности Fono — диктовка, словарь и обработка текста",
    description:
      "Узнайте, как Fono помогает работать с текстом: голосовой ввод, личный словарь, история, обработка с ИИ, голосовые команды и речевой тренажёр.",
  },
  {
    id: "download",
    route: "download",
    title: "Скачать Fono бесплатно для Windows",
    description:
      "Бесплатное приложение Fono для Windows x64. Статус установщика и первые шаги: модель распознавания, микрофон и горячая клавиша.",
  },
  {
    id: "help",
    route: "help",
    title: "Помощь Fono — начало работы и ответы на вопросы",
    description:
      "Как начать диктовку в Fono, выбрать модель, настроить микрофон и обработку текста. Ответы на частые вопросы о голосовом вводе для Windows.",
  },
  {
    id: "privacy",
    route: "privacy",
    title: "Приватность Fono — локальное распознавание и ваши данные",
    description:
      "Где Fono обрабатывает аудио, как сохраняется история и когда текст передаётся внешнему ИИ. Понятное описание настроек приватности.",
  },
];

function publicUrl(value, name, allowPath) {
  if (!value) return "";
  const url = new URL(value);
  if (
    url.protocol !== "https:" ||
    url.username ||
    url.password ||
    (!allowPath && (url.search || url.hash))
  ) {
    throw new Error(`${name}: ожидается публичный HTTPS URL без credentials.`);
  }
  if (!allowPath) url.pathname = `${url.pathname.replace(/\/+$/, "")}/`;
  return url.href;
}

export function getConfig(overrides = {}) {
  const config = {
    ...settings,
    siteUrl: process.env.FONO_SITE_URL ?? settings.siteUrl,
    downloadUrl: process.env.FONO_DOWNLOAD_URL ?? settings.downloadUrl,
    ...overrides,
  };
  return {
    siteUrl: publicUrl(config.siteUrl, "siteUrl", false),
    downloadUrl: publicUrl(config.downloadUrl, "downloadUrl", true),
  };
}

export function linksFor(page) {
  const links = Object.fromEntries(
    pages.map((target) => {
      const relative = path.posix.relative(page.route, target.route);
      return [target.id, relative ? `${relative}/` : "./"];
    }),
  );
  return { links, assets: page.route ? "../assets/" : "assets/" };
}

export function metadataFor(page, assets, config) {
  const tags = [
    `<meta name="robots" content="${config.siteUrl ? "index, follow" : "noindex, nofollow"}">`,
    '<meta property="og:type" content="website">',
    '<meta property="og:locale" content="ru_RU">',
    '<meta property="og:site_name" content="Fono">',
    `<meta property="og:title" content="${escapeHtml(page.title)}">`,
    `<meta property="og:description" content="${escapeHtml(page.description)}">`,
    `<meta property="og:image" content="${escapeHtml(config.siteUrl ? new URL("assets/images/og-cover.png", config.siteUrl).href : `${assets}images/og-cover.png`)}">`,
    '<meta property="og:image:alt" content="Логотип Fono — голосовой ввод для Windows">',
    '<meta property="og:image:width" content="1024">',
    '<meta property="og:image:height" content="1024">',
  ];
  if (config.siteUrl) {
    const url = escapeHtml(
      new URL(page.route ? `${page.route}/` : "./", config.siteUrl).href,
    );
    tags.push(`<link rel="canonical" href="${url}">`);
    tags.push(`<meta property="og:url" content="${url}">`);
  }
  return tags.join("\n");
}

export function downloadPanel(config) {
  if (config.downloadUrl) {
    return `<div class="download-panel card"><span class="chip">Бесплатно · Windows x64</span><h2>Fono готов к скачиванию</h2><p>Скачайте установщик и начните с настройки микрофона и модели распознавания.</p><a class="button button-primary" href="${escapeHtml(config.downloadUrl)}">Скачать бесплатно для Windows</a></div>`;
  }
  return '<div class="download-panel card"><span class="chip">Бесплатно · Windows x64</span><h2>Бесплатный установщик готовится к публикации</h2><p>Когда установщик будет опубликован, здесь появится кнопка скачивания. Публичные сборки будут доступны в GitHub Releases.</p><a class="button button-secondary" href="https://github.com/whitewolf06/fono/releases">Открыть GitHub Releases <span aria-hidden="true">↗</span></a></div>';
}
