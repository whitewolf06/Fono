import { createWakeSamples } from "../application/recording";
import type {
  WakeSample,
  WakeSampleTest,
  WakeSamplesPort,
  LiraLiveSnapshot,
} from "../domain/samples";
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export function createNativeWakeSamples(call: Call): WakeSamplesPort {
  const port = createWakeSamples({
    liveStatus: () => call<LiraLiveSnapshot>("get_lira_live_status"),
    enableLive: () => call<LiraLiveSnapshot>("start_lira_live"),
    disableLive: (sessionId) =>
      call<LiraLiveSnapshot>("stop_lira_live", { sessionId }),
    testAvailable: () => call<boolean>("wake_sample_test_available"),
    test: (id) => call<WakeSampleTest>("test_wake_sample", { id }),
    list: () => call<WakeSample[]>("list_wake_samples"),
    start: (request) => call("start_wake_sample", { request }),
    finish: (sessionId) => call("finish_wake_sample", { sessionId }),
    cancel: (sessionId) => call("cancel_wake_sample", { sessionId }),
    async audio(id) {
      const encoded = await call<string>("read_wake_sample", { id });
      const bytes = Uint8Array.from(atob(encoded), (value) =>
        value.charCodeAt(0),
      );
      const url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
      return { url, release: () => URL.revokeObjectURL(url) };
    },
    remove: (id) => call("delete_wake_sample", { id }),
  });
  const hidden = () => {
    if (globalThis.document?.visibilityState === "hidden") void port.cancel();
  };
  globalThis.document?.addEventListener?.("visibilitychange", hidden);
  return {
    ...port,
    dispose() {
      globalThis.document?.removeEventListener?.("visibilitychange", hidden);
      port.dispose();
    },
  };
}
