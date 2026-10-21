import { message as uiMessage } from "../../domain/messages";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { createEventHub } from "./eventHub";
import { isBackendEventV1, type BackendEventV1 } from "./backendEvents";
import {
  NativeIpcError,
  normalizeNativeEvent,
  normalizeNativeResult,
} from "./nativeErrors";
export const isDesktop = isTauri;
const commandCounts = new Map<string, number>();
const events = createEventHub(async (channel, handler) =>
  listen(channel, (event) =>
    handler(normalizeNativeEvent(channel, event.payload)),
  ),
);
/** Development-only numeric counters; arguments and session text are never kept. */
export const ipcDebugCounters = {
  snapshot: () => ({
    invocations: [...commandCounts.values()].reduce((sum, n) => sum + n, 0),
    commands: Object.fromEntries(commandCounts),
    channels: events.channelCount(),
  }),
  reset: () => commandCounts.clear(),
};
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (import.meta.env.DEV)
    commandCounts.set(command, (commandCounts.get(command) || 0) + 1);
  try {
    return normalizeNativeResult(command, await invoke<T>(command, args)) as T;
  } catch (error) {
    throw new NativeIpcError(error);
  }
}
export async function subscribe<T>(
  channel: string,
  handler: (value: T) => void,
): Promise<UnlistenFn> {
  return events.subscribe(channel, handler);
}
export function subscribeBackend(handler: (event: BackendEventV1) => void) {
  return subscribe<unknown>("backend-event-v1", (value) => {
    if (isBackendEventV1(value)) handler(value);
  });
}
export async function playMicrophoneSample() {
  const samples = await call<number[]>("get_microphone_sample");
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const text = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i++)
      view.setUint8(offset + i, value.charCodeAt(i));
  };
  text(0, "RIFF");
  view.setUint32(4, buffer.byteLength - 8, true);
  text(8, "WAVE");
  text(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, 16000, true);
  view.setUint32(28, 32000, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, samples.length * 2, true);
  samples.forEach((sample, i) => view.setInt16(44 + i * 2, sample, true));
  const url = URL.createObjectURL(new Blob([buffer], { type: "audio/wav" }));
  const audio = new Audio(url);
  try {
    await new Promise<void>((resolve, reject) => {
      audio.onended = () => resolve();
      audio.onerror = () => reject(new Error(uiMessage("ui.a41a5065bc")));
      void audio.play().catch(reject);
    });
  } finally {
    URL.revokeObjectURL(url);
  }
}
