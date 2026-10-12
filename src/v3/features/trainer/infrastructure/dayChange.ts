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
  return schedule(() => {
    const next = new Date(now()).toISOString().slice(0, 10);
    if (next !== day) {
      day = next;
      handler();
    }
  });
}
