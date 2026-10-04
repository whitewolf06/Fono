import type {
  ProcessingPreset,
  TranslationLanguage,
} from "../../../shared/domain/processing";

export type IndicatorPhase =
  "recording" | "transcribing" | "processing" | "ready" | "error" | "closed";
export interface IndicatorChoice {
  postprocessingOn: boolean;
  translationOn: boolean;
  style: ProcessingPreset;
  language: TranslationLanguage;
}
/** Presentation state shared by the real adapter and browser preview. */
export interface IndicatorState extends IndicatorChoice {
  phase: IndicatorPhase;
  sessionId: number;
  elapsedMs: number;
  level: number;
  result: string;
  error: string;
  copying: boolean;
  status?: string;
}
