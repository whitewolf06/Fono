import type { PendingDictation } from "../../domain/processing";
import type { NativeLiveSnapshot, NativeSnapshot } from "./dictationContracts";
import { call } from "./ipc";

interface Handlers {
  operation(): number;
  pending(value: PendingDictation | null): void;
  snapshot(value: NativeSnapshot): void;
  live(value: NativeLiveSnapshot | null): void;
}

/** A response started before a newer event/action never overwrites that event. */
export function createNativeSessionSync(handlers: Handlers) {
  let eventRevision = 0;
  let requestGeneration = 0;
  function invalidate() {
    return ++eventRevision;
  }
  async function refresh() {
    const revision = eventRevision;
    const generation = ++requestGeneration;
    const [snapshot, live, pending] = await Promise.all([
      call<NativeSnapshot>("get_desktop_snapshot"),
      call<NativeLiveSnapshot | null>("get_live_dictation"),
      call<PendingDictation | null>("get_pending_dictation"),
    ]);
    if (
      revision !== eventRevision ||
      generation !== requestGeneration ||
      (snapshot.operation_id > 0 &&
        snapshot.operation_id < handlers.operation())
    )
      return;
    handlers.live(live);
    // These getters can observe adjacent native transitions. A pending session
    // older than the observed capture cannot take ownership of the newer one.
    handlers.pending(
      pending && pending.sessionId < snapshot.operation_id ? null : pending,
    );
    handlers.snapshot(snapshot);
  }
  return { invalidate, refresh };
}
