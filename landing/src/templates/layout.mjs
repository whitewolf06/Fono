import { escapeHtml } from "./html.mjs";

function navigationLink(href, label, id, pageId, className = "") {
  const current = id === pageId ? ' aria-current="page"' : "";
  return `<a href="${escapeHtml(href)}"${current}${className ? ` class="${className}"` : ""}>${label}</a>`;
}

export function renderLayout({ page, content, assets, links, metadata }) {
  const assetPath = escapeHtml(assets);
  const styles = [
    "tokens",
    "base",
    "layout",
    "components",
    "home",
    "illustration",
    "infographic",
    "infographic-responsive",
    "tables",
    "pages",
  ]
    .map((name) => `<link rel="stylesheet" href="${assetPath}css/${name}.css">`)
    .join("\n    ");
  const brand = `<img class="brand-icon" src="${assetPath}images/fono-icon.png" width="34" height="34" alt=""><img class="brand-wordmark" src="${assetPath}images/fono-wordmark.svg" width="91" height="30" alt="Fono">`;

  return `<!doctype html>
<html lang="ru">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="color-scheme" content="dark">
    <meta name="theme-color" content="#071321">
    <title>${escapeHtml(page.title)}</title>
    <meta name="description" content="${escapeHtml(page.description)}">
    ${metadata}
    <link rel="icon" href="${assetPath}favicon.ico" sizes="any">
    <link rel="icon" type="image/png" href="${assetPath}favicon.png" sizes="32x32">
    ${styles}
    <script type="module" src="${assetPath}js/navigation.js"></script>
  </head>
  <body>
    <a class="skip-link" href="#main-content">Перейти к содержимому</a>
    <header class="site-header">
      <div class="container header-inner">
        <a class="brand" href="${escapeHtml(links.home)}" aria-label="Fono — главная">${brand}</a>
        <button class="menu-toggle" type="button" data-menu-toggle aria-controls="site-nav" aria-expanded="false" hidden>
          <svg aria-hidden="true"><use href="${assetPath}images/icons.svg#menu"></use></svg>
          Меню
        </button>
        <nav class="site-nav" id="site-nav" data-site-nav aria-label="Основная навигация">
          ${navigationLink(links.features, "Возможности", "features", page.id)}
          ${navigationLink(links.download, "Скачать", "download", page.id, "nav-download")}
          ${navigationLink(links.help, "Помощь", "help", page.id)}
          ${navigationLink(links.privacy, "Приватность", "privacy", page.id)}
        </nav>
        <a class="button button-secondary header-download" href="${escapeHtml(links.download)}">Скачать Fono<svg aria-hidden="true"><use href="${assetPath}images/icons.svg#arrow-right"></use></svg></a>
      </div>
    </header>
    <main id="main-content" tabindex="-1">
      ${content}
    </main>
    <footer class="site-footer">
      <div class="container">
        <div class="footer-top">
          <div class="footer-brand">
            <a class="brand" href="${escapeHtml(links.home)}" aria-label="Fono — главная">${brand}</a>
            <p>Ваш голос. Ваши мысли.<br>Ваш текст на компьютере.</p>
          </div>
          <nav class="footer-links" aria-label="Навигация в подвале">
            <a href="${escapeHtml(links.features)}">Возможности</a>
            <a href="${escapeHtml(links.download)}">Скачать для Windows</a>
            <a href="${escapeHtml(links.help)}">Помощь</a>
            <a href="${escapeHtml(links.privacy)}">Приватность</a>
            <a href="https://github.com/whitewolf06/fono">GitHub проекта</a>
            <a href="https://github.com/whitewolf06/fono/releases">Релизы на GitHub</a>
          </nav>
        </div>
        <div class="footer-bottom">
          <span>Fono · Голосовой ввод для Windows</span>
          <span>Создано для ваших повседневных задач.</span>
        </div>
      </div>
    </footer>
  </body>
</html>
`;
}
