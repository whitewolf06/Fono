export type WakeLanguage = "ru" | "en";
export type WakePhraseId = "privet_lira" | "hey_fono";
export type WakeSampleKind = "phrase" | "other_speech" | "background";
export type WakeCondition = "quiet" | "noisy";
export interface WakeSampleRequest {
  phraseId: WakePhraseId;
  language: WakeLanguage;
  kind: WakeSampleKind;
  condition: WakeCondition;
  consent: boolean;
}
export interface WakeSample {
  id: string;
  phraseId: WakePhraseId;
  language: WakeLanguage;
  kind: WakeSampleKind;
  condition: WakeCondition;
  durationMs: number;
  sha256: string;
  recordedAtMs: number;
}
export const wakeSampleSets = [
  {
    id: "privet_lira",
    phraseId: "privet_lira",
    language: "ru",
    phrase: "Привет, Лира",
    archived: false,
  },
  {
    id: "hey_fono_ru",
    phraseId: "hey_fono",
    language: "ru",
    phrase: "Эй, Фоно",
    archived: true,
  },
  {
    id: "hey_fono_en",
    phraseId: "hey_fono",
    language: "en",
    phrase: "Hey Fono",
    archived: true,
  },
] as const;
export function wakeSamplesInSet(
  samples: WakeSample[],
  request: WakeSampleRequest,
): WakeSample[] {
  return samples.filter(
    (sample) =>
      (sample.phraseId ?? "hey_fono") === request.phraseId &&
      sample.language === request.language &&
      sample.kind === request.kind &&
      sample.condition === request.condition,
  );
}
export interface WakeSamplesState {
  samples: WakeSample[];
  phase: "idle" | "starting" | "recording" | "saving" | "cancelling";
  remainingMs: number;
  error: string;
  audio: { id: string; url: string } | null;
  testAvailable: boolean;
  testingId: string | null;
  testResults: Record<string, WakeSampleTest>;
  live: LiraLiveSnapshot;
  livePending: boolean;
}
export interface LiraLiveSnapshot {
  available: boolean;
  sessionId: number | null;
  phase: "off" | "loading" | "listening" | "paused" | "stopping" | "error";
  message: string;
}
export const liraOff = (): LiraLiveSnapshot => ({
  available: false,
  sessionId: null,
  phase: "off",
  message: "",
});
export const liraActive = (live: LiraLiveSnapshot) =>
  ["loading", "listening", "paused", "stopping"].includes(live.phase);
export interface WakeSampleTest {
  operationId: string;
  detected: boolean;
  proposals: number;
  events: number;
  processingMs: number;
  durationMs: number;
}
export interface WakeSamplesPort {
  readonly state: WakeSamplesState;
  load(): Promise<void>;
  refreshLive(): Promise<void>;
  record(request: WakeSampleRequest): Promise<void>;
  finish(): Promise<void>;
  cancel(): Promise<void>;
  listen(id: string): Promise<void>;
  remove(id: string): Promise<void>;
  test(id: string): Promise<void>;
  enableLive(): Promise<void>;
  disableLive(): Promise<void>;
  dispose(): void;
}
export interface WakeSamplesBackend {
  liveStatus(): Promise<LiraLiveSnapshot>;
  enableLive(): Promise<LiraLiveSnapshot>;
  disableLive(sessionId: number): Promise<LiraLiveSnapshot>;
  testAvailable(): Promise<boolean>;
  test(id: string): Promise<WakeSampleTest>;
  list(): Promise<WakeSample[]>;
  start(
    request: WakeSampleRequest,
  ): Promise<{ sessionId: number; durationMs: number }>;
  finish(sessionId: number): Promise<WakeSample>;
  cancel(sessionId: number): Promise<void>;
  audio(id: string): Promise<{ url: string; release(): void }>;
  remove(id: string): Promise<void>;
}
