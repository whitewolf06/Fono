import { createWakeSamples } from "../application/recording";
import { message } from "../../../shared/domain/messages";
import { liraOff } from "../domain/samples";
export function createMockWakeSamples() {
  const unavailable = async (): Promise<never> => {
    throw new Error(message("wakeSamples.desktopOnly"));
  };
  return createWakeSamples({
    liveStatus: async () => liraOff(),
    enableLive: unavailable,
    disableLive: async () => liraOff(),
    testAvailable: async () => false,
    test: unavailable,
    list: async () => [],
    start: unavailable,
    finish: unavailable,
    cancel: async () => {},
    audio: unavailable,
    remove: unavailable,
  });
}
