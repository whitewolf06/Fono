import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
export const isDesktop = isTauri;
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw new Error(
      typeof error === "string"
        ? error
        : error instanceof Error
          ? error.message
          : JSON.stringify(error),
    );
  }
}
export async function subscribe<T>(
  channel: string,
  handler: (value: T) => void,
): Promise<UnlistenFn> {
  return listen<T>(channel, (event) => handler(event.payload));
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
      audio.onerror = () =>
        reject(new Error("Не удалось воспроизвести образец"));
      void audio.play().catch(reject);
    });
  } finally {
    URL.revokeObjectURL(url);
  }
}
