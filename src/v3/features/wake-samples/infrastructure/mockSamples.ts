import { createWakeSamples } from "../application/recording";
import { message } from "../../../shared/domain/messages";
export function createMockWakeSamples() {
  const unavailable = async (): Promise<never> => {
    throw new Error(message("wakeSamples.desktopOnly"));
  };
  return createWakeSamples({
    list: async () => [],
    start: unavailable,
    finish: unavailable,
    cancel: async () => {},
    audio: unavailable,
    remove: unavailable,
  });
}
