import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";

const history = [
  {
    app: "Visual Studio Code",
    text: "Подготовь, пожалуйста, письмо клиенту и добавь уточнение по срокам релиза.",
    time: "12 сек назад",
  },
  {
    app: "Telegram",
    text: "Обсудим детали на созвоне завтра в одиннадцать часов.",
    time: "13:49",
  },
  {
    app: "Почта",
    text: "Здравствуйте! Направляю отчёт по проекту за текущую неделю.",
    time: "12:17",
  },
];

export function DictationHistoryPanel({ onClose }: { onClose: () => void }) {
  const [query, setQuery] = useState("");
  const visibleEntries = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();

    if (!normalizedQuery) return history;

    return history.filter(({ app, text }) =>
      `${app} ${text}`.toLocaleLowerCase().includes(normalizedQuery),
    );
  }, [query]);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };

    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  return createPortal(
    <div
      className="v2-history-backdrop"
      role="presentation"
      onMouseDown={onClose}
    >
      <aside
        className="v2-history-panel"
        aria-label="История диктовок"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header>
          <div>
            <span>Последние 7 дней</span>
            <h2>История диктовок</h2>
          </div>
          <button type="button" aria-label="Закрыть" onClick={onClose}>
            ×
          </button>
        </header>
        <label className="v2-history-panel__search">
          <span>Поиск по тексту и приложению</span>
          <input
            value={query}
            placeholder="Найти диктовку"
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <div className="v2-history-panel__list">
          {visibleEntries.map((entry) => (
            <article key={`${entry.app}-${entry.time}`}>
              <div>
                <time>{entry.time}</time>
                <strong>{entry.app}</strong>
              </div>
              <p>{entry.text}</p>
              <button type="button">Вставить повторно</button>
            </article>
          ))}
          {!visibleEntries.length && (
            <p className="v2-history-panel__empty">Ничего не найдено.</p>
          )}
        </div>
      </aside>
    </div>,
    document.body,
  );
}
