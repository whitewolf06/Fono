import type { Dictation } from "../../../shared/domain/contracts";
import type { TrainerSnapshot, TrainerSpeechTiming } from "./report";

export const emptySpeechTiming = (): TrainerSpeechTiming => ({
  measuredSessions: 0,
  unknownSessions: 0,
  words: 0,
  speechTimeMs: 0,
  pauseCount: 0,
  pauseTimeMs: 0,
  maxPauseMs: 0,
});

export function addSpeechTiming(
  total: TrainerSpeechTiming,
  entry: Dictation,
  words: number,
) {
  const t = entry.metadata?.speechTiming;
  const integers =
    t &&
    [t.speechTimeMs, t.pauseCount, t.pauseTimeMs, t.maxPauseMs].every(
      (v) => Number.isSafeInteger(v) && v >= 0,
    );
  const valid =
    integers &&
    t.schemaVersion === 1 &&
    t.speechTimeMs > 0 &&
    t.speechTimeMs + t.pauseTimeMs <= 86_400_000 &&
    (entry.metadata?.recordingDurationMs == null ||
      t.speechTimeMs + t.pauseTimeMs <= entry.metadata.recordingDurationMs) &&
    t.maxPauseMs <= t.pauseTimeMs &&
    (t.pauseCount === 0
      ? t.pauseTimeMs === 0 && t.maxPauseMs === 0
      : t.maxPauseMs > 0 && t.pauseTimeMs >= t.pauseCount);
  if (!valid || !t) {
    total.unknownSessions++;
    return;
  }
  total.measuredSessions++;
  total.words += words;
  total.speechTimeMs += t.speechTimeMs;
  total.pauseCount += t.pauseCount;
  total.pauseTimeMs += t.pauseTimeMs;
  total.maxPauseMs = Math.max(total.maxPauseMs, t.maxPauseMs);
}

export function speechTimingMetrics(value?: TrainerSpeechTiming | null) {
  const known = !!value && value.measuredSessions > 0 && value.speechTimeMs > 0;
  return {
    wordsPerMinute: known ? (value.words * 60_000) / value.speechTimeMs : null,
    pausePercent: known
      ? (value.pauseTimeMs * 100) / (value.speechTimeMs + value.pauseTimeMs)
      : null,
    averagePauseMs: known
      ? value.pauseCount > 0
        ? value.pauseTimeMs / value.pauseCount
        : 0
      : null,
    maxPauseMs: known ? value.maxPauseMs : null,
  };
}

export function withoutSpeechTiming(
  snapshot: TrainerSnapshot,
): TrainerSnapshot {
  return {
    ...snapshot,
    speechTiming: null,
    daily: snapshot.daily.map((day) => ({ ...day, speechTiming: null })),
  };
}
