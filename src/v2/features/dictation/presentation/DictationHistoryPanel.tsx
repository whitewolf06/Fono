import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import type { DictationHistoryEntry } from "@/lib/types";
import type { DictationHistoryStore } from "../infrastructure/tauriDictationHistoryStore";

export function DictationHistoryPanel({ onClose, store }: { onClose: () => void; store: DictationHistoryStore }) {
  const [query, setQuery] = useState("");
  const [entries, setEntries] = useState<DictationHistoryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { void store.load().then(setEntries, (cause) => setError(cause instanceof Error ? cause.message : "Не удалось загрузить историю.")); }, [store]);
  const visibleEntries = useMemo(() => {
    const term = query.trim().toLocaleLowerCase();
    return term ? entries.filter((entry) => `${entry.device ?? ""} ${entry.text}`.toLocaleLowerCase().includes(term)) : entries;
  }, [entries, query]);
  useEffect(() => { const close = (event: KeyboardEvent) => event.key === "Escape" && onClose(); window.addEventListener("keydown", close); return () => window.removeEventListener("keydown", close); }, [onClose]);
  return createPortal(<div className="v2-history-backdrop" role="presentation" onMouseDown={onClose}><aside className="v2-history-panel" aria-label="История диктовок" onMouseDown={(event) => event.stopPropagation()}><header><div><span>Локально на этом устройстве</span><h2>История диктовок</h2></div><button type="button" aria-label="Закрыть" onClick={onClose}>×</button></header><label className="v2-history-panel__search"><span>Поиск по тексту и устройству</span><input value={query} placeholder="Найти диктовку" onChange={(event) => setQuery(event.target.value)} /></label><div className="v2-history-panel__list">{visibleEntries.map((entry) => <article key={entry.id}><div><time>{new Date(entry.created_at).toLocaleString()}</time><strong>{entry.device ?? "Fono"}</strong></div><p>{entry.text}</p></article>)}{error && <p className="v2-history-panel__empty">{error}</p>}{!error && !visibleEntries.length && <p className="v2-history-panel__empty">История пока пуста.</p>}</div></aside></div>, document.body);
}
