import type { WorkspaceState } from "../../../shared/domain/contracts";
import { assertDraftAvailable } from "../domain/live";
import { processDemoText } from "./mockText";

export function createMockDraftActions(
  state: WorkspaceState,
  delay: (milliseconds: number) => Promise<void>,
) {
  function assertAvailable() {
    assertDraftAvailable(state.live);
    if (state.pendingDictation)
      throw new Error("Сначала вставьте или отмените ожидающую диктовку.");
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
        throw new Error("В живом режиме обработка через ИИ отключена.");
      if (!preferences.processingEnabled || !state.last.draft.trim())
        throw new Error(
          "Включите обработку текста, чтобы использовать улучшение.",
        );
      const entryId = state.last.entry?.id;
      const previous = state.last.draft;
      await delay(800);
      if (!state.aiAvailable)
        throw new Error(
          "Модель недоступна. Текст не изменён — проверьте подключение.",
        );
      if (state.last.entry?.id !== entryId || state.last.draft !== previous)
        return;
      state.last.undo = previous;
      state.last.draft = processDemoText(
        previous,
        preferences.processingMode,
        preferences.processingTranslation === "none"
          ? null
          : preferences.processingTranslation,
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
