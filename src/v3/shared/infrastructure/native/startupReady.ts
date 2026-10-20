interface ReadinessPort {
  listen(ready: () => void): Promise<() => void>;
  read(): Promise<boolean>;
}

/** Subscribe before reading; one startup fence, without steady IPC polling. */
export async function waitForDesktopReady(
  port: ReadinessPort,
  timeoutMs = 15000,
): Promise<void> {
  let finished = false;
  let unlisten: (() => void) | undefined;
  let ready!: () => void;
  const signal = new Promise<void>((resolve) => {
    ready = resolve;
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error("Fono runtime startup timed out")),
      timeoutMs,
    );
  });
  const connect = async () => {
    const release = await port.listen(ready);
    if (finished) {
      release();
      return;
    }
    unlisten = release;
    await Promise.race([
      signal,
      port.read().then((value) => (value ? undefined : signal)),
    ]);
  };
  try {
    await Promise.race([connect(), timeout]);
  } finally {
    finished = true;
    clearTimeout(timer);
    unlisten?.();
  }
}
