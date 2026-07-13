import type { DictationSnapshot, ReadinessSnapshot } from "@/v2/shared/domain/pipeline";

export interface DictationRuntime {
  getSnapshot(): Promise<DictationSnapshot>;
  getReadiness(): Promise<ReadinessSnapshot>;
  start(): Promise<void>;
  stop(): Promise<void>;
  subscribe(listener: (snapshot: DictationSnapshot) => void): () => void;
}
