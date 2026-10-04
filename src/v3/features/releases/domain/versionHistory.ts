/** Repository changes. A version entry is not evidence of a published release. */
export interface VersionEntry {
  version: string;
  date: string;
  title: string;
  changes: string[];
}
export interface VersionSeries {
  id: string;
  title: string;
  description: string;
  entries: VersionEntry[];
}
export function compareVersions(left: string, right: string): number {
  const a = left.split(".").map(Number);
  const b = right.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const difference = (a[i] || 0) - (b[i] || 0);
    if (difference) return difference;
  }
  return 0;
}
export function formatVersionDate(date: string): string {
  return new Date(`${date}T12:00:00Z`).toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
}
