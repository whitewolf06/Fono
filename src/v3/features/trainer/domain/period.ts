import type { Dictation, Preferences } from "../../../shared/domain/contracts";
import { transcriptWords } from "./analysis";
import type {
  TrainerCounts,
  TrainerFindingKind,
  TrainerPeriod,
  TrainerSnapshot,
  TrainerTerm,
} from "./report";

export const TRAINER_RULES_VERSION = "speech-patterns-1";
const DAY = 86_400_000;
const fillers = new Set([
  "ну",
  "вот",
  "типа",
  "короче",
  "значит",
  "ээ",
  "эм",
  "мм",
]);
const corrections = ["то есть", "вернее", "точнее", "в смысле", "ой"];
const emptyCounts = (): TrainerCounts => ({
  filler: 0,
  repetition: 0,
  selfCorrection: 0,
  unfinished: 0,
});
const countKey = (kind: TrainerFindingKind): keyof TrainerCounts =>
  kind === "self_correction" ? "selfCorrection" : kind;
const markerCount = (counts: TrainerCounts) =>
  counts.filler + counts.repetition + counts.selfCorrection + counts.unfinished;

export function trainerRange(days: TrainerPeriod, now: number) {
  const date = new Date(now);
  const midnight = Date.UTC(
    date.getUTCFullYear(),
    date.getUTCMonth(),
    date.getUTCDate(),
  );
  return { from: midnight - (days - 1) * DAY, to: midnight + DAY };
}

// Preview-only identity. Native snapshots use SHA-256; browser demo data never persists.
export function demoFingerprint(value: unknown): string {
  const text = JSON.stringify(value);
  let hash = 2166136261;
  for (let i = 0; i < text.length; i++)
    hash = Math.imul(hash ^ text.charCodeAt(i), 16777619);
  return "demo-" + (hash >>> 0).toString(16);
}

function sourceEntries(entries: Dictation[], days: TrainerPeriod, now: number) {
  const range = trainerRange(days, now);
  return entries
    .filter((entry) => {
      const time = Date.parse(entry.createdAt);
      return time >= range.from && time < range.to;
    })
    .sort((a, b) => a.id.localeCompare(b.id));
}

export function trainerFingerprint(
  entries: Dictation[],
  preferences: Preferences,
  days: TrainerPeriod,
  now: number,
) {
  return demoFingerprint([
    TRAINER_RULES_VERSION,
    trainerRange(days, now),
    preferences.historyEnabled,
    preferences.retentionDays,
    preferences.analyticsConsent,
    preferences.trainerEnabled,
    sourceEntries(entries, days, now).map((entry) => [
      entry.id,
      entry.createdAt,
      entry.analyticsIncluded !== false,
      entry.analysisReady !== false,
      entry.original ?? null,
      Date.parse(entry.createdAt) >= now - preferences.retentionDays * DAY,
    ]),
  ]);
}

export function buildTrainerSnapshot(
  entries: Dictation[],
  preferences: Preferences,
  days: TrainerPeriod,
  now: number,
): TrainerSnapshot {
  const range = trainerRange(days, now);
  const report: TrainerSnapshot = {
    fingerprint: trainerFingerprint(entries, preferences, days, now),
    rulesVersion: TRAINER_RULES_VERSION,
    days,
    from: new Date(range.from).toISOString(),
    to: new Date(range.to).toISOString(),
    generatedAt: new Date(now).toISOString(),
    includedSessions: 0,
    excludedSessions: 0,
    pendingSessions: 0,
    unavailableSessions: 0,
    totalWords: 0,
    counts: emptyCounts(),
    density: 0,
    terms: [],
    sessionIds: [],
    insufficientData: true,
    historyLimit: null,
    coverageStart:
      entries
        .map((entry) => Date.parse(entry.createdAt))
        .filter(Number.isFinite)
        .sort((a, b) => a - b)
        .map((time) => new Date(time).toISOString())[0] ?? null,
    retentionDays: preferences.retentionDays,
    daily: Array.from({ length: days }, (_, index) => ({
      date: new Date(range.from + index * DAY).toISOString().slice(0, 10),
      sessions: 0,
      words: 0,
      counts: emptyCounts(),
      density: 0,
    })),
  };
  const terms = new Map<string, TrainerTerm>();
  for (const entry of sourceEntries(entries, days, now)) {
    if (entry.analyticsIncluded === false) {
      report.excludedSessions++;
      continue;
    }
    if (
      !preferences.historyEnabled ||
      !preferences.analyticsConsent ||
      !preferences.trainerEnabled ||
      !entry.original ||
      Date.parse(entry.createdAt) < now - preferences.retentionDays * DAY
    ) {
      report.unavailableSessions++;
      continue;
    }
    if (entry.analysisReady === false) {
      report.pendingSessions++;
      continue;
    }
    report.includedSessions++;
    report.sessionIds.push(entry.id);
    const words = transcriptWords(entry.original);
    const normalized = words.map((word) =>
      word.normalize("NFKC").toLowerCase().replaceAll("ё", "е"),
    );
    const day = report.daily.find(
      (item) =>
        item.date === new Date(entry.createdAt).toISOString().slice(0, 10),
    )!;
    day.sessions++;
    day.words += words.length;
    report.totalWords += words.length;
    const add = (
      kind: TrainerFindingKind,
      label: string,
      start: number,
      length: number,
    ) => {
      const key = `${kind}:${kind === "unfinished" ? "unfinished" : label}`;
      let term = terms.get(key);
      if (!term) {
        term = {
          key,
          kind,
          label,
          count: 0,
          density: 0,
          affectedSessions: 0,
          sessionIds: [],
          examples: [],
        };
        terms.set(key, term);
      }
      term.count++;
      if (!term.sessionIds.includes(entry.id)) {
        term.sessionIds.push(entry.id);
        term.affectedSessions++;
        if (term.examples.length < 3)
          term.examples.push({
            sessionId: entry.id,
            fragment: words.slice(start, start + length).join(" "),
          });
      }
      const category = countKey(kind);
      report.counts[category]++;
      day.counts[category]++;
    };
    for (let i = 0; i < words.length; i++) {
      if (normalized[i] === "как" && normalized[i + 1] === "бы") {
        add("filler", "как бы", i, 2);
        i++;
      } else if (fillers.has(normalized[i])) add("filler", normalized[i], i, 1);
    }
    for (let i = 0; i < words.length; i++) {
      if (i > 0 && normalized[i] === normalized[i - 1])
        add("repetition", normalized[i], i - 1, 2);
      for (const phrase of corrections) {
        const length = phrase.split(" ").length;
        if (normalized.slice(i, i + length).join(" ") === phrase)
          add("self_correction", phrase, i, length);
      }
    }
    if (words.length && /(?:…|\.\.\.|-)\s*$/.test(entry.original))
      add("unfinished", "Незавершённая фраза", words.length - 1, 1);
  }
  report.density = report.totalWords
    ? (markerCount(report.counts) * 100) / report.totalWords
    : 0;
  report.insufficientData =
    report.includedSessions < 3 || report.totalWords < 100;
  report.terms = [...terms.values()]
    .map((term) => ({
      ...term,
      density: report.totalWords ? (term.count * 100) / report.totalWords : 0,
    }))
    .sort((a, b) => b.count - a.count || a.key.localeCompare(b.key));
  for (const day of report.daily)
    day.density = day.words ? (markerCount(day.counts) * 100) / day.words : 0;
  return report;
}
