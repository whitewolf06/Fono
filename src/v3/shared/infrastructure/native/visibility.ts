/** Browser/window access stays in the native adapter. Show + focus coalesce. */
export function bindNativeVisibility(onShow: () => void) {
  let disposed = false;
  let queued = false;
  const visible = () =>
    typeof document === "undefined" || document.visibilityState !== "hidden";
  function changed() {
    if (disposed || !visible() || queued) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      if (!disposed && visible()) onShow();
    });
  }
  globalThis.document?.addEventListener?.("visibilitychange", changed);
  globalThis.window?.addEventListener?.("focus", changed);
  return {
    visible,
    dispose() {
      disposed = true;
      globalThis.document?.removeEventListener?.("visibilitychange", changed);
      globalThis.window?.removeEventListener?.("focus", changed);
    },
  };
}
