// Browser clock access stays in an adapter. The callback only refreshes local data.
export function observeTrainerDay(
  handler: () => void,
  now: () => number = Date.now,
  schedule: (tick: () => void) => () => void = (tick) => {
    const timer = setInterval(tick, 60_000);
    return () => clearInterval(timer);
  },
): () => void {
  let day = new Date(now()).toISOString().slice(0, 10);
  const tick = () => {
    if (
      typeof document !== "undefined" &&
      document.visibilityState === "hidden"
    )
      return;
    const next = new Date(now()).toISOString().slice(0, 10);
    if (next !== day) {
      day = next;
      handler();
    }
  };
  const release = schedule(tick);
  if (typeof document !== "undefined")
    document.addEventListener?.("visibilitychange", tick);
  return () => {
    release();
    if (typeof document !== "undefined")
      document.removeEventListener?.("visibilitychange", tick);
  };
}
