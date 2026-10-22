import type { Dictation } from "../../../shared/domain/contracts";
import { message, type MessageKey } from "../../../shared/domain/messages";

export function contextLabel(entry: Dictation): string {
  const c = entry.applicationContext;
  return message(
    `applicationContext.${c?.outcome ?? "not_collected"}` as MessageKey,
  );
}
export function filterByApplication(
  entries: Dictation[],
  value: string,
): Dictation[] {
  if (!value) return entries;
  return entries.filter((e) =>
    value === "not_collected"
      ? !e.applicationContext
      : value === "unknown"
        ? !!e.applicationContext?.attempted && !e.applicationContext.before
        : e.applicationContext?.before?.id === value,
  );
}
export function applicationChoices(entries: Dictation[]) {
  const apps = new Map<string, string>();
  for (const e of entries)
    if (e.applicationContext?.before)
      apps.set(
        e.applicationContext.before.id,
        e.applicationContext.before.name,
      );
  return [...apps]
    .map(([value, label]) => ({ value, label }))
    .sort((a, b) => a.label.localeCompare(b.label));
}
export function applicationUsage(entries: Dictation[]) {
  const rows = new Map<
    string,
    { id: string; name: string; attempts: number; share: number }
  >();
  let total = 0;
  for (const e of entries) {
    const c = e.applicationContext;
    if (!c?.attempted) continue;
    total++;
    const app = c.before ?? {
      id: "unknown",
      name: message("applicationContext.unknown"),
    };
    const row = rows.get(app.id) ?? { ...app, attempts: 0, share: 0 };
    row.attempts++;
    rows.set(app.id, row);
  }
  for (const row of rows.values()) row.share = total ? row.attempts / total : 0;
  return {
    total,
    rows: [...rows.values()].sort(
      (a, b) => b.attempts - a.attempts || a.name.localeCompare(b.name),
    ),
  };
}
