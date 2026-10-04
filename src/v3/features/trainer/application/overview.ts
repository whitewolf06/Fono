import type { Dictation } from "../../../shared/domain/contracts";
import { analyze, transcriptWords } from "../domain/analysis";

export type TrainerSort = "date" | "duration" | "length";
export type ChartMetric = "count" | "density";

export function recordingDuration(entry: Dictation): number | undefined {
  const measured = entry.metadata?.recordingDurationMs;
  if (measured != null)
    return Number.isFinite(measured) && measured >= 0 ? measured : undefined;
  return Number.isFinite(entry.duration) && entry.duration > 0
    ? entry.duration * 1000
    : undefined;
}

export function recordingDurationLabel(entry: Dictation): string {
  const duration = recordingDuration(entry);
  if (duration == null) return "Длительность не сохранена";
  const seconds = Math.round(duration / 1000);
  if (seconds < 60) return `${seconds} с`;
  return `${Math.floor(seconds / 60)} мин ${String(seconds % 60).padStart(2, "0")} с`;
}

export function wordCount(entry: Dictation): number {
  return transcriptWords(entry.original ?? entry.text).length;
}

export function findingsCount(entry: Dictation): number {
  return analyze(entry).reduce((sum, item) => sum + item.count, 0);
}

export function exactDateLabel(date: string): string {
  const value = new Date(date);
  return Number.isFinite(value.getTime())
    ? value.toLocaleString("ru-RU", {
        day: "2-digit",
        month: "2-digit",
        year: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : "Дата не сохранена";
}

export function sortTrainerEntries(
  entries: Dictation[],
  sort: TrainerSort,
): Dictation[] {
  const timestamp = (entry: Dictation) => Date.parse(entry.createdAt) || 0;
  return entries.slice().sort((a, b) => {
    const metric = sort === "duration" ? recordingDuration : wordCount;
    const difference =
      sort === "date" ? 0 : (metric(b) ?? -1) - (metric(a) ?? -1);
    return (
      difference || timestamp(b) - timestamp(a) || a.id.localeCompare(b.id)
    );
  });
}

export function aggregateFindings(entries: Dictation[]) {
  const totals = new Map<string, number>();
  for (const entry of entries)
    for (const finding of analyze(entry))
      totals.set(
        finding.title,
        (totals.get(finding.title) || 0) + finding.count,
      );
  return [...totals].map(([title, count]) => ({ title, count }));
}

export function chartEntries(entries: Dictation[], limit = 24): Dictation[] {
  return sortTrainerEntries(entries, "date").slice(0, limit).reverse();
}

export function chartValue(
  count: number,
  entry: Dictation,
  metric: ChartMetric,
): number {
  const words = wordCount(entry);
  return metric === "density" ? (words > 0 ? (count * 100) / words : 0) : count;
}

export function chartScale(values: number[]): { max: number; ticks: number[] } {
  const largest = Math.max(0, ...values);
  const roughStep = Math.max(1, largest / 4);
  const magnitude = 10 ** Math.floor(Math.log10(roughStep));
  const step =
    [1, 2, 5, 10].find((value) => value * magnitude >= roughStep)! * magnitude;
  const max = Math.max(step, Math.ceil(largest / step) * step);
  const ticks = Array.from(
    { length: Math.round(max / step) + 1 },
    (_, i) => i * step,
  );
  return { max, ticks };
}

export function formatMetric(value: number): string {
  return new Intl.NumberFormat("ru-RU", { maximumFractionDigits: 1 }).format(
    value,
  );
}
