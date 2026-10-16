type Release = () => void;
type Handler = (value: unknown) => void;
type Listen = (channel: string, handler: Handler) => Promise<Release>;

/** One native listener per channel in this webview, including pending listens. */
export function createEventHub(listen: Listen) {
  const channels = new Map<
    string,
    { handlers: Set<Handler>; ready: Promise<Release> }
  >();
  async function subscribe<T>(channel: string, handler: (value: T) => void) {
    const receive: Handler = (value) => handler(value as T);
    let entry = channels.get(channel);
    if (!entry) {
      const handlers = new Set<Handler>([receive]);
      entry = {
        handlers,
        ready: Promise.resolve().then(() =>
          listen(channel, (value) => {
            for (const callback of [...handlers]) {
              if (!handlers.has(callback)) continue;
              try {
                callback(value);
              } catch {
                console.error("Fono event handler failed");
              }
            }
          }),
        ),
      };
      channels.set(channel, entry);
    } else entry.handlers.add(receive);
    const current = entry;
    let release: Release;
    try {
      release = await current.ready;
    } catch (error) {
      current.handlers.delete(receive);
      if (channels.get(channel) === current) channels.delete(channel);
      throw error;
    }
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      current.handlers.delete(receive);
      if (current.handlers.size || channels.get(channel) !== current) return;
      channels.delete(channel);
      release();
    };
  }
  return { subscribe, channelCount: () => channels.size };
}
