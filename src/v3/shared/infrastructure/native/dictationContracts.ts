import type { PipelineState } from "./ipcTypes";
import type { LiveDictation } from "../../domain/contracts";

export interface NativeResult {
  id: string;
  text: string;
  original_text: string;
  created_at: string;
  audio_secs: number;
}
export interface NativeSnapshot {
  state: PipelineState;
  level: number;
  last: NativeResult | null;
  operation_id: number;
  source: string | null;
}
export interface NativeLiveSnapshot {
  session_id: string;
  revision: number;
  committed_text: string;
  draft_text: string;
  pending_text: string;
  insertion_state: LiveDictation["insertionState"];
  lag_ms: number;
  phase: LiveDictation["phase"];
  source: "ui" | "hotkey" | "wake_word";
  elapsed_ms: number;
  audio_level: number;
  warning?: string | null;
}
