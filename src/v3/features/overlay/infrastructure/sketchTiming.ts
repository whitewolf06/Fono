/** Timing adapter for the browser sketch; no audio or desktop operations. */
export const sketchTiming = {
  wait: (milliseconds: number) =>
    new Promise<void>((resolve) => setTimeout(resolve, milliseconds)),
  subscribeTick(callback: () => void, intervalMs: number) {
    const timer = setInterval(callback, intervalMs);
    return () => clearInterval(timer);
  },
};
