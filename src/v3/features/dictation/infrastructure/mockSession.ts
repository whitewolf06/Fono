import type {
  Dictation,
  Preferences,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import type { ProcessingPreset } from "../../../shared/domain/processing";

export interface CaptureSnapshot {
  preferences: Preferences;
  modelLabel: string;
}

export function capturePreferences(state: WorkspaceState): CaptureSnapshot {
  return {
    modelLabel:
      state.models.find((model) => model.id === state.preferences.model)
        ?.name || state.preferences.model,
    preferences: {
      ...state.preferences,
      dictionaryEntries: state.preferences.dictionaryEntries.map((entry) => ({
        ...entry,
        spoken: [...entry.spoken],
      })),
    },
  };
}

export function selectLatest(state: WorkspaceState, entry: Dictation | null) {
  state.last = {
    entry,
    variant: entry?.original ? "original" : "result",
    draft: entry?.original ?? entry?.text ?? "",
    edited: false,
    undo: null,
  };
}

export function createDemoEntry(
  state: WorkspaceState,
  snapshot: CaptureSnapshot,
  original: string,
  sessionId: number,
  generationStarted: number,
  recognitionDurationMs: number,
): Dictation {
  const preferences = snapshot.preferences;
  return {
    id: `session-${sessionId}-${generationStarted}`,
    createdAt: new Date().toISOString(),
    title: "Новая диктовка · демо",
    original,
    text: original,
    duration: Math.max(1, Math.round(state.elapsed)),
    metadata: {
      demo: true,
      recordingDurationMs: Math.round(state.elapsed * 1000),
      generationDurationMs: Date.now() - generationStarted,
      recognitionDurationMs,
      processingDurationMs: null,
      model: snapshot.modelLabel,
      language: preferences.language,
      requestedAcceleration: ["auto", "cpu", "cuda", "vulkan"].includes(
        preferences.acceleration,
      )
        ? (preferences.acceleration as "auto" | "cpu" | "cuda" | "vulkan")
        : undefined,
      backend: null,
      dictationMode: preferences.dictationMode,
      processingMode: "off",
      dictionaryEnabled: preferences.dictionaryEnabled,
    },
  };
}

export function publishDemoEntry(
  state: WorkspaceState,
  snapshot: CaptureSnapshot,
  entry: Dictation,
  text: string,
  processingMode: ProcessingPreset | "off",
  generationStarted: number,
  processingDurationMs: number | null,
) {
  entry.text = text;
  Object.assign(entry.metadata!, {
    processingMode,
    generationDurationMs: Date.now() - generationStarted,
    processingDurationMs,
  });
  selectLatest(state, entry);
  if (text !== entry.original) {
    state.last.variant = "result";
    state.last.draft = text;
  }
  const preferences = snapshot.preferences;
  if (preferences.historyEnabled) {
    const archived = { ...entry, metadata: { ...entry.metadata } };
    if (!preferences.analyticsConsent || !preferences.trainerEnabled)
      delete archived.original;
    state.history.unshift(archived);
  }
}
