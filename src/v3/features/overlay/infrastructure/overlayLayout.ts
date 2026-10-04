import { watch } from "vue";
import type { Phase, Preferences } from "../../../shared/domain/contracts";
import type { PendingDictation } from "../../../shared/domain/processing";
import { call } from "../../../shared/infrastructure/native/ipc";

interface LayoutState {
  preferences: Preferences;
  phase: Phase;
  pending: PendingDictation | null;
  helpOpen: boolean;
  error: string;
}
export function bindOverlayLayout(
  state: LayoutState,
  processingError: () => string,
  fail: (error: unknown) => void,
) {
  let disposed = false,
    running = false,
    queued = false;
  async function resize() {
    queued = true;
    if (running) return;
    running = true;
    try {
      while (!disposed && queued) {
        queued = false;
        try {
          await call("set_overlay_layout", {
            layout: {
              helpOpen: state.helpOpen,
              errorVisible: !!(state.error || processingError()),
            },
          });
        } catch (error) {
          if (!disposed) fail(error);
        }
      }
    } finally {
      running = false;
    }
  }
  const stop = watch(
    [
      () => state.helpOpen,
      () => state.phase,
      () => state.pending?.sessionId,
      () => state.pending?.phase,
      () => state.preferences.overlayCompact,
      () => state.preferences.overlayScale,
      () => state.preferences.overlayQuickProcessing,
      () => state.preferences.processingEnabled,
      () => state.preferences.hotkeyMode,
      () => !!(state.error || processingError()),
    ],
    () => {
      void resize();
    },
    { immediate: true },
  );
  return () => {
    disposed = true;
    stop();
  };
}
