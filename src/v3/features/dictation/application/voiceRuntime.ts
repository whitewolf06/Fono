import type { VoiceOverview, VoicePhase } from "../domain/voice";

export interface VoiceRuntime {
  readonly demo: boolean;
  load(): Promise<VoiceOverview>;
  start(): Promise<void>;
  stop(): Promise<void>;
  toggleWakeWord(): Promise<void>;
  copyText(text: string): Promise<void>;
  subscribePhase(listener: (phase: VoicePhase) => void): () => void;
}
