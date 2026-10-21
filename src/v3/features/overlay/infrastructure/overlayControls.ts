import { message as uiMessage } from "../../../shared/domain/messages";
import { watch } from "vue";
import type { Settings } from "../../../shared/infrastructure/native/ipcTypes";
import type { Phase, Preferences } from "../../../shared/domain/contracts";
import type {
  OverlayProcessingChoice,
  PendingDictation,
  PendingDictationRequest,
} from "../../../shared/domain/processing";
import { createOverlayProcessingQueue } from "./overlayProcessingChoice";

interface State {
  preferences: Preferences;
  phase: Phase;
  sessionId: number | null;
  preview: unknown;
  pending: PendingDictation | null;
  processingChoice: OverlayProcessingChoice | null;
  sessionChoice?: OverlayProcessingChoice | null;
}
export function createOverlayControls(
  state: State,
  settings: () => Settings | null,
  commitSettings: (settings: Settings) => void,
  disposed: () => boolean,
) {
  function context(): number | null | undefined {
    if (state.pending?.phase === "awaiting_action")
      return state.pending.sessionId;
    if (["listening", "silence"].includes(state.phase))
      return state.sessionId ?? undefined;
    if (state.preview && state.phase === "idle") return null;
    return undefined;
  }
  const queue = createOverlayProcessingQueue({
    readConfirmed: () => {
      if (state.sessionChoice && context() !== null && context() !== undefined)
        return { ...state.sessionChoice };
      const s = settings();
      return s
        ? {
            preset:
              s.processing_preset ??
              (s.ai_mode === "format" ? "format" : "clean"),
            targetLanguage: s.processing_target_language ?? null,
            processingEnabled: s.ai_mode !== "off",
            translationEnabled: s.processing_translation_enabled,
            recognitionLanguage: s.language,
          }
        : {
            preset: state.preferences.processingMode,
            targetLanguage:
              state.preferences.processingTranslation === "none"
                ? null
                : state.preferences.processingTranslation,
            processingEnabled: state.preferences.processingEnabled,
            translationEnabled: state.preferences.processingTranslationEnabled,
            recognitionLanguage: state.preferences.language,
          };
    },
    isCurrent: (id) => !disposed() && context() === id,
    commit: (s, request) => {
      commitSettings(s);
      state.processingChoice = {
        ...(request.recognitionLanguage === undefined
          ? {}
          : { recognitionLanguage: s.language }),
        ...(request.processingEnabled === undefined
          ? {}
          : { processingEnabled: request.processingEnabled }),
        ...(request.translationEnabled === undefined
          ? {}
          : { translationEnabled: request.translationEnabled }),
        preset: request.preset,
        targetLanguage: request.targetLanguage,
      };
      if (state.sessionChoice)
        state.sessionChoice = {
          ...state.sessionChoice,
          ...state.processingChoice,
        };
    },
    rollback: (_error, confirmed) => {
      state.processingChoice = confirmed;
    },
  });
  const release = watch(
    context,
    () => {
      queue.discard();
      state.processingChoice = null;
    },
    { flush: "sync" },
  );
  return {
    state: queue.state,
    choose(choice: OverlayProcessingChoice) {
      const sessionId = context();
      if (sessionId === undefined || disposed()) return;
      state.processingChoice = { ...choice };
      queue.update({ ...choice, sessionId });
    },
    async finish(action: () => Promise<unknown>) {
      const id = context();
      if (id === undefined) return;
      const recording = ["listening", "silence"].includes(state.phase);
      await queue.flush();
      if (
        !disposed() &&
        context() === id &&
        (recording
          ? ["listening", "silence"].includes(state.phase)
          : state.phase === "idle")
      )
        await action();
    },
    async flushFor(sessionId: number) {
      if (!queue.state.pending && !queue.state.saving && !queue.state.error)
        return;
      if (disposed() || context() !== sessionId)
        throw new Error(uiMessage("ui.df80913eda"));
      await queue.flush();
      if (disposed() || context() !== sessionId)
        throw new Error(uiMessage("ui.df80913eda"));
    },
    async resolve(
      request: PendingDictationRequest,
      action: (request: PendingDictationRequest) => Promise<void>,
    ) {
      if (disposed() || state.pending?.sessionId !== request.sessionId) return;
      if (["process_and_insert", "process_preview"].includes(request.action))
        await queue.flush();
      else queue.discard();
      if (!disposed() && state.pending?.sessionId === request.sessionId)
        await action(request);
    },
    cancel() {
      queue.discard();
    },
    dispose() {
      release();
      queue.dispose();
    },
  };
}
