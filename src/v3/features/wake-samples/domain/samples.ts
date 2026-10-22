export type WakeLanguage = "ru" | "en";
export type WakeSampleKind = "phrase" | "other_speech" | "background";
export type WakeCondition = "quiet" | "noisy";
export interface WakeSampleRequest {
  language: WakeLanguage;
  kind: WakeSampleKind;
  condition: WakeCondition;
  consent: boolean;
}
export interface WakeSample {
  id: string;
  language: WakeLanguage;
  kind: WakeSampleKind;
  condition: WakeCondition;
  durationMs: number;
  sha256: string;
  recordedAtMs: number;
}
export interface WakeSamplesState {
  samples: WakeSample[];
  phase: "idle" | "starting" | "recording" | "saving" | "cancelling";
  remainingMs: number;
  error: string;
  audio: { id: string; url: string } | null;
}
export interface WakeSamplesPort {
  readonly state: WakeSamplesState;
  load(): Promise<void>;
  record(request: WakeSampleRequest): Promise<void>;
  finish(): Promise<void>;
  cancel(): Promise<void>;
  listen(id: string): Promise<void>;
  remove(id: string): Promise<void>;
  dispose(): void;
}
export interface WakeSamplesBackend {
  list(): Promise<WakeSample[]>;
  start(
    request: WakeSampleRequest,
  ): Promise<{ sessionId: number; durationMs: number }>;
  finish(sessionId: number): Promise<WakeSample>;
  cancel(sessionId: number): Promise<void>;
  audio(id: string): Promise<{ url: string; release(): void }>;
  remove(id: string): Promise<void>;
}
