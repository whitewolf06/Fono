export type TrainerPeriod = 7 | 30 | 90;
export type TrainerFindingKind =
  "filler" | "repetition" | "self_correction" | "unfinished";

export interface TrainerCounts {
  filler: number;
  repetition: number;
  selfCorrection: number;
  unfinished: number;
}
export interface TrainerTerm {
  key: string;
  label: string;
  kind: TrainerFindingKind;
  count: number;
  density: number;
  affectedSessions: number;
  sessionIds: string[];
  examples: { sessionId: string; fragment: string }[];
}
export interface TrainerDay {
  speechTiming?: TrainerSpeechTiming | null;
  date: string;
  sessions: number;
  words: number;
  counts: TrainerCounts;
  density: number;
}
export interface TrainerSnapshot {
  speechTiming?: TrainerSpeechTiming | null;
  fingerprint: string;
  rulesVersion: string;
  days: TrainerPeriod;
  from: string;
  to: string;
  generatedAt: string;
  includedSessions: number;
  excludedSessions: number;
  pendingSessions: number;
  unavailableSessions: number;
  totalWords: number;
  counts: TrainerCounts;
  density: number;
  terms: TrainerTerm[];
  daily: TrainerDay[];
  sessionIds: string[];
  insufficientData: boolean;
  historyLimit: number | null;
  coverageStart: string | null;
  retentionDays: number;
}
export interface TrainerSpeechTiming {
  measuredSessions: number;
  unknownSessions: number;
  words: number;
  speechTimeMs: number;
  pauseCount: number;
  pauseTimeMs: number;
  maxPauseMs: number;
}
export interface TrainerAiSummary {
  fingerprint: string;
  snapshotFingerprint: string;
  generatedAt: string;
  profileId: string;
  profileName: string;
  model: string;
  dataScope: "metrics_only" | "findings" | "original_text";
  context?: TrainerAiContext | null;
  summary: string;
  priorities: { title: string; basis: string; exercise: string }[];
}
export interface TrainerAiContext {
  dataScope: TrainerAiSummary["dataScope"];
  payloadBytes: number;
  payloadByteLimit: number;
  reduced: boolean;
  metricsSessions: number;
  metricsWords: number;
  availableSessions: number;
  sharedSessions: number;
  omittedSessions: number;
  shortenedSessions: number;
  availableWords: number;
  sharedWords: number;
  omittedWords: number;
  availableCharacters: number;
  sharedCharacters: number;
  omittedCharacters: number;
  patternsAvailable: number;
  patternsShared: number;
  examplesAvailable: number;
  examplesShared: number;
  limits: {
    maxSharedSessions: number;
    maxCharactersPerSession: number;
    maxPatterns: number;
    maxExamplesPerPattern: number;
  };
}
export interface TrainerAiConfiguration {
  enabled: boolean;
  profileId: string | null;
  profileName: string | null;
  model: string | null;
  cloud: boolean;
  cloudConsent: boolean;
  dataScope: "metrics_only" | "findings" | "original_text";
}
export interface TrainerReport {
  snapshot: TrainerSnapshot;
  currentFingerprint: string;
  stale: boolean;
  aiFingerprint: string;
  aiStale: boolean;
  aiSummary: TrainerAiSummary | null;
  aiConfiguration: TrainerAiConfiguration;
  aiDisabledReason: string | null;
  aiContext?: {
    metricsOnly: TrainerAiContext;
    configuredShared: TrainerAiContext | null;
  } | null;
}
export interface TrainerPeriodAnalysis {
  days: TrainerPeriod;
  fingerprint: string;
  shareText: boolean;
}
