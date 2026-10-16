/** One active snapshot and one queued latest read; pushed values can retire replies. */
export function createCoalescedRead<T>(
  read: () => Promise<T>,
  apply: (value: T) => void,
  active: () => boolean = () => true,
) {
  let generation = 0;
  let queued = false;
  let disposed = false;
  let running: Promise<void> | undefined;
  const current = () => !disposed && active();

  async function drain() {
    try {
      while (queued && current()) {
        queued = false;
        const ticket = generation;
        try {
          const value = await read();
          if (ticket === generation && current()) apply(value);
        } catch (error) {
          if (ticket === generation && current()) throw error;
        }
      }
    } finally {
      // Clear ownership before this async function settles. A separate Promise.finally
      // would leave a gap where a new request joins an already completed drain.
      running = undefined;
    }
  }

  function refresh(): Promise<void> {
    if (!current()) return Promise.resolve();
    generation++;
    queued = true;
    running ??= Promise.resolve().then(drain);
    return running;
  }

  return {
    refresh,
    invalidate() {
      generation++;
    },
    dispose() {
      disposed = true;
      queued = false;
      generation++;
    },
  };
}
