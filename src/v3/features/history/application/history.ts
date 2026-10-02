import type { Dictation } from "../../../shared/domain/contracts";
export function filterHistory(
  entries: Dictation[],
  query: string,
  period: number,
  now = Date.now(),
): Dictation[] {
  const needle = query.trim().toLocaleLowerCase("ru");
  return entries.filter(
    (e) =>
      (!period || now - Date.parse(e.createdAt) <= period * 86400000) &&
      (e.text + " " + (e.original ?? "") + " " + e.title)
        .toLocaleLowerCase("ru")
        .includes(needle),
  );
}
export function groupHistory(
  entries: Dictation[],
): { date: string; entries: Dictation[] }[] {
  const grouped = new Map<string, Dictation[]>();
  for (const entry of entries) {
    const date = new Date(entry.createdAt).toLocaleDateString("ru-RU", {
      day: "numeric",
      month: "long",
      year: "numeric",
    });
    grouped.set(date, [...(grouped.get(date) ?? []), entry]);
  }
  return [...grouped].map(([date, items]) => ({ date, entries: items }));
}
export function timeLabel(date: string) {
  return new Date(date).toLocaleTimeString("ru-RU", {
    hour: "2-digit",
    minute: "2-digit",
  });
}
