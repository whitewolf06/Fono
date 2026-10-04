import { computed, onScopeDispose, ref, type Ref } from "vue";
import type { Preferences } from "../../../shared/domain/contracts";
import type { ProcessingPreviewResult } from "../../../shared/domain/processing";
import { useWorkspace } from "../../../shared/application/workspace";
export function useProcessingPreview(draft: Ref<Preferences>) {
  const workspace = useWorkspace();
  const input = ref("");
  const original = ref("");
  const result = ref<ProcessingPreviewResult | null>(null);
  const busy = ref(false);
  const capturing = ref(false);
  const captureBusy = ref(false);
  const error = ref("");
  const notice = ref("");
  let disposed = false;
  let captureRequested = false;
  const targetLanguage = computed(() =>
    draft.value.processingTranslationEnabled &&
    draft.value.processingTranslation !== "none"
      ? draft.value.processingTranslation
      : null,
  );
  const usesModel = computed(
    () => draft.value.processingMode !== "raw" || targetLanguage.value !== null,
  );
  const connectionChanged = computed(
    () =>
      usesModel.value &&
      (draft.value.profile !== workspace.state.preferences.profile ||
        draft.value.processingModel !==
          workspace.state.preferences.processingModel),
  );
  const activeDictation = computed(() =>
    [
      "listening",
      "silence",
      "transcribing",
      "processing",
      "awaiting_action",
    ].includes(workspace.state.phase),
  );
  const connectionMissing = computed(
    () =>
      usesModel.value &&
      (!workspace.state.preferences.profile ||
        !workspace.state.preferences.processingModel),
  );
  const canTest = computed(
    () =>
      input.value.trim().length > 0 &&
      !busy.value &&
      !capturing.value &&
      !captureBusy.value &&
      !connectionChanged.value &&
      !connectionMissing.value,
  );
  async function attempt(action: () => Promise<void>) {
    error.value = "";
    notice.value = "";
    try {
      await action();
    } catch (cause) {
      if (!disposed)
        error.value = cause instanceof Error ? cause.message : String(cause);
    }
  }
  async function test() {
    if (!canTest.value) return;
    const text = input.value;
    busy.value = true;
    result.value = null;
    await attempt(async () => {
      const preset = draft.value.processingMode;
      const value = await workspace.settings.previewProcessing({
        text,
        preset,
        targetLanguage: targetLanguage.value,
        ...(preset !== "raw"
          ? { promptOverride: { ...draft.value.processingPrompts[preset] } }
          : {}),
      });
      if (!disposed) {
        original.value = text;
        result.value = value;
      }
    });
    busy.value = false;
  }
  function useLast() {
    input.value =
      workspace.state.last.entry?.original ||
      workspace.state.last.draft ||
      workspace.state.last.entry?.text ||
      "";
  }
  async function startCapture() {
    if (
      captureBusy.value ||
      busy.value ||
      capturing.value ||
      activeDictation.value
    )
      return;
    captureBusy.value = true;
    captureRequested = true;
    await attempt(async () => {
      await workspace.settings.startProcessingCapture();
      if (!disposed) capturing.value = true;
    });
    captureBusy.value = false;
  }
  async function finishCapture() {
    if (!capturing.value || captureBusy.value) return;
    captureBusy.value = true;
    await attempt(async () => {
      const text = await workspace.settings.finishProcessingCapture();
      if (!disposed) {
        if (text.trim()) input.value = text;
        else
          notice.value =
            "Речь не обнаружена. Попробуйте надиктовать пример ещё раз.";
      }
    });
    capturing.value = false;
    captureRequested = false;
    captureBusy.value = false;
  }
  async function cancelCapture() {
    if (!capturing.value || captureBusy.value) return;
    captureBusy.value = true;
    await attempt(() => workspace.settings.cancelProcessingCapture());
    capturing.value = false;
    captureRequested = false;
    captureBusy.value = false;
  }
  async function copy() {
    if (result.value) await attempt(() => workspace.copy(result.value!.text));
  }
  onScopeDispose(() => {
    disposed = true;
    if (captureRequested)
      void workspace.settings.cancelProcessingCapture().catch(() => {});
  });
  return {
    workspace,
    input,
    original,
    result,
    busy,
    capturing,
    captureBusy,
    error,
    notice,
    targetLanguage,
    usesModel,
    connectionChanged,
    connectionMissing,
    activeDictation,
    canTest,
    test,
    useLast,
    startCapture,
    finishCapture,
    cancelCapture,
    copy,
  };
}
