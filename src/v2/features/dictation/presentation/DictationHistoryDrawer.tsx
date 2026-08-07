import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import type { DictationHistoryEntry } from "@/lib/types";
import type { DictationHistoryStore } from "../infrastructure/tauriDictationHistoryStore";

interface Props {
  onClose: () => void;
  store: DictationHistoryStore;
}

export function DictationHistoryDrawer({ onClose, store }: Props) {
  const [entries, setEntries] = useState<DictationHistoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busyEntryId, setBusyEntryId] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void store.load().then(
      (history) => active && setEntries(history),
      (cause: unknown) =>
        active && setError(messageFrom(cause, "Не удалось загрузить историю.")),
    );
    return () => {
      active = false;
    };
  }, [store]);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  const visibleEntries = useMemo(() => {
    const term = query.trim().toLocaleLowerCase();
    if (!term) return entries;
    return entries.filter(({ device, text }) =>
      `${device ?? ""} ${text}`.toLocaleLowerCase().includes(term),
    );
  }, [entries, query]);

  const reinsert = async (entry: DictationHistoryEntry) => {
    setBusyEntryId(entry.id);
    setError(null);
    try {
      await store.reinsert(entry.text);
      onClose();
    } catch (cause) {
      setError(messageFrom(cause, "Не удалось вставить текст."));
    } finally {
      setBusyEntryId(null);
    }
  };

  const copy = async (entry: DictationHistoryEntry) => {
    setBusyEntryId(entry.id);
    setError(null);
    try {
      await store.copy(entry.text);
    } catch (cause) {
      setError(messageFrom(cause, "Не удалось скопировать текст."));
    } finally {
      setBusyEntryId(null);
    }
  };

  const remove = async (entry: DictationHistoryEntry) => {
    setBusyEntryId(entry.id);
    setError(null);
    try {
      await store.delete(entry.id);
      setEntries((current) => current.filter(({ id }) => id !== entry.id));
    } catch (cause) {
      setError(messageFrom(cause, "Не удалось удалить запись."));
    } finally {
      setBusyEntryId(null);
    }
  };

  const clear = async () => {
    setError(null);
    try {
      await store.clear();
      setEntries([]);
    } catch (cause) {
      setError(messageFrom(cause, "Не удалось очистить историю."));
    }
  };

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
            <span>Локально на этом устройстве</span>
            <h2>История диктовок</h2>
          </div>
          <button type="button" aria-label="Закрыть" onClick={onClose}>
            ×
          </button>
        </header>
        <label className="v2-history-panel__search">
          <span>Поиск по тексту и устройству</span>
          <input
            value={query}
            placeholder="Найти диктовку"
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <div className="v2-history-panel__list">
          {visibleEntries.map((entry) => (
            <article key={entry.id}>
              <div>
                <time>{new Date(entry.created_at).toLocaleString()}</time>
                <strong>{entry.device ?? "Fono"}</strong>
              </div>
              <p>{entry.text}</p>
              <div className="v2-history-panel__actions">
                <button
                  type="button"
                  disabled={busyEntryId === entry.id}
                  onClick={() => void copy(entry)}
                >
                  Копировать
                </button>
                <button
                  type="button"
                  disabled={busyEntryId === entry.id}
                  onClick={() => void reinsert(entry)}
                >
                  Вставить повторно
                </button>
                <button
                  className="is-danger"
                  type="button"
                  disabled={busyEntryId === entry.id}
                  onClick={() => void remove(entry)}
                >
                  Удалить
                </button>
              </div>
            </article>
          ))}
          {!error && !visibleEntries.length && (
            <p className="v2-history-panel__empty">История пока пуста.</p>
          )}
          {error && <p className="v2-history-panel__empty">{error}</p>}
        </div>
        {!!entries.length && (
          <button
            className="v2-button"
            type="button"
            onClick={() => void clear()}
          >
            Очистить историю
          </button>
        )}
      </aside>
    </div>,
    document.body,
  );
}

function messageFrom(cause: unknown, fallback: string) {
  return cause instanceof Error ? cause.message : fallback;
}
