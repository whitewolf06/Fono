import { message as uiMessage } from "../../../shared/domain/messages";
import type { WorkspaceState } from "../../../shared/domain/contracts";
import { assertDraftAvailable } from "../domain/live";
import { processDemoText } from "./mockText";

export function createMockDraftActions(
  state: WorkspaceState,
  delay: (milliseconds: number) => Promise<void>,
) {
  function assertAvailable() {
    assertDraftAvailable(state.live);
    if (state.pendingDictation) throw new Error(uiMessage("ui.e4e045a4e4"));
  }
  return {
    chooseVariant(variant: "original" | "result") {
      assertAvailable();
      const entry = state.last.entry;
      if (!entry) return;
      state.last.variant = variant;
      state.last.draft =
        variant === "original" ? (entry.original ?? entry.text) : entry.text;
      state.last.edited = false;
      state.last.undo = null;
    },
    edit(text: string) {
      assertAvailable();
      state.last.draft = text;
      state.last.edited = true;
      state.last.undo = null;
    },
    async improve() {
      assertAvailable();
      const preferences = { ...state.preferences };
      if (preferences.dictationMode === "live")
        throw new Error(uiMessage("ui.6035ff10fb"));
      if (!preferences.processingEnabled || !state.last.draft.trim())
        throw new Error(uiMessage("ui.ea5df82a05"));
      const entryId = state.last.entry?.id;
      const previous = state.last.draft;
      const targetLanguage =
        preferences.processingTranslationEnabled !== false &&
        preferences.processingTranslation !== "none"
          ? preferences.processingTranslation
          : null;
      const needsModel = preferences.processingMode !== "raw" || targetLanguage;
      if (needsModel) await delay(800);
      if (needsModel && !state.aiAvailable)
        throw new Error(uiMessage("ui.dce066c693"));
      if (state.last.entry?.id !== entryId || state.last.draft !== previous)
        return;
      state.last.undo = previous;
      state.last.draft = processDemoText(
        previous,
        preferences.processingMode,
        targetLanguage,
      );
      state.last.edited = true;
    },
    undoImprove() {
      if (state.last.undo !== null) {
        state.last.draft = state.last.undo;
        state.last.undo = null;
      }
    },
  };
}
