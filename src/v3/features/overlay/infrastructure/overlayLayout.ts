import { nextTick, watch } from "vue";
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
    queued = false;
  let running: Promise<void> | null = null;
  let contentHeight: number | null = null;
  let revision = 0;
  let appliedRevision = -1;
  let observer: ResizeObserver | null = null;
  let widget: Element | null = null;

  function measure() {
    if (disposed || appliedRevision !== revision || !widget) return;
    // The rectangle includes CSS zoom. Applying user scale again would double it.
    const height = Math.ceil(widget.getBoundingClientRect().height);
    if (
      !Number.isFinite(height) ||
      height < 32 ||
      height > 1600 ||
      height === contentHeight
    )
      return;
    contentHeight = height;
    void resize();
  }
  function observe() {
    if (
      disposed ||
      typeof document === "undefined" ||
      typeof document.querySelector !== "function"
    )
      return;
    const nextWidget = document.querySelector(
      ".native-overlay .overlay-indicator",
    );
    if (nextWidget !== widget) {
      observer?.disconnect();
      widget = nextWidget;
      if (widget && typeof ResizeObserver !== "undefined") {
        observer = new ResizeObserver(measure);
        observer.observe(widget);
      }
    }
    measure();
  }
  function resize() {
    queued = true;
    if (!running) running = flush();
    return running;
  }
  async function flush() {
    try {
      while (!disposed && queued) {
        queued = false;
        const currentRevision = revision;
        const measured = contentHeight;
        try {
          await call("set_overlay_layout", {
            layout: {
              helpOpen: state.helpOpen,
              errorVisible: !!(state.error || processingError()),
              contentHeight: measured,
            },
          });
          if (!disposed && currentRevision === revision) {
            appliedRevision = currentRevision;
            await nextTick();
            observe();
          }
        } catch (error) {
          if (!disposed) fail(error);
        }
      }
    } finally {
      running = null;
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
      // Give a newly expanded panel space while its natural height is measured.
      contentHeight = null;
      revision++;
      void resize();
    },
    { immediate: true },
  );
  return () => {
    disposed = true;
    observer?.disconnect();
    stop();
  };
}
