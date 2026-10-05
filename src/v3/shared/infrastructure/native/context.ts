import type { WorkspaceState, Preferences } from "../../domain/contracts";
import type {
  Settings,
  WhisperModelInfo,
  DeviceInfo,
  WakeWordCapabilities,
  LocalTranscriptionServiceSnapshot,
} from "./ipcTypes";
import { call } from "./ipc";
import type { GpuMemoryStatus } from "../../domain/gpuMemory";
import {
  preferencesFromNative,
  profilesFromNative,
  modelsFromNative,
  historyFromNative,
  jobFromNative,
  applyPreferences,
} from "./mapping";
import type { DictationHistoryEntry } from "./ipcTypes";
export function createNativeContext(state: WorkspaceState) {
  let models: WhisperModelInfo[] = [];
  let saveTail: Promise<unknown> = Promise.resolve();
  let historyInitialized = false;
  function serialize<T>(action: () => Promise<T>): Promise<T> {
    const task = saveTail.catch(() => {}).then(action);
    saveTail = task;
    return task;
  }
  function report(error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    state.error = message;
    state.logs.unshift(message);
  }
  async function readSettings() {
    const raw = await call<Settings>("get_settings");
    state.preferences = preferencesFromNative(raw, models);
    state.profiles = profilesFromNative(raw);
    if (
      raw.whisper_model_path &&
      !models.some((m) => m.local_path === raw.whisper_model_path)
    ) {
      const selected = await call<{
        path: string;
        filename: string;
        bytes: number;
      } | null>("get_selected_model_metadata");
      state.models = state.models.filter((m) => !m.custom);
      if (selected)
        state.models.push({
          id: selected.path,
          name: selected.filename,
          size: Math.round(selected.bytes / 1048576) + " МБ",
          description: "Модель из выбранного файла",
          status: "installed",
          progress: 100,
          custom: true,
        });
    }
    state.applications = raw.launch_apps.map((a, i) => ({
      id: String(i),
      name: a.name,
      path: a.exe_path,
      phrase: a.aliases.join(", "),
    }));
    return raw;
  }
  async function readModels() {
    models = await call<WhisperModelInfo[]>("list_whisper_models");
    const downloading = state.models.filter((m) => m.status === "downloading");
    state.models = modelsFromNative(models).map((m) =>
      m.status === "installed"
        ? m
        : downloading.find((d) => d.id === m.id) || m,
    );
  }
  async function readHistory() {
    state.history = (
      await call<DictationHistoryEntry[]>("get_dictation_history")
    ).map(historyFromNative);
    if (!historyInitialized && !state.last.entry && state.history.length) {
      const entry = state.history[0];
      state.last = {
        entry,
        variant: "result",
        draft: entry.text,
        edited: false,
        undo: null,
      };
    }
    historyInitialized = true;
  }
  async function readDevices() {
    const devices = await call<DeviceInfo[]>("list_audio_devices");
    state.devices = [
      { value: "system", label: "Системный микрофон" },
      ...devices.map((d) => ({ value: d.id, label: d.name })),
    ];
    state.microphoneAvailable = devices.some((d) =>
      state.preferences.microphone === "system"
        ? d.is_default
        : d.id === state.preferences.microphone,
    );
  }
  async function readService() {
    const service = await call<
      LocalTranscriptionServiceSnapshot & {
        enabled: boolean;
        error: string | null;
      }
    >("get_local_transcription_service_snapshot");
    state.serviceAddress = service.address;
    state.serviceError = service.error || "";
    if (!state.pending.serviceEnabled)
      state.preferences.serviceEnabled = service.enabled;
    state.jobs = [
      ...new Map(
        [...service.history.jobs, ...service.queue.jobs].map((j) => [j.id, j]),
      ).values(),
    ]
      .sort((a, b) => b.created_at_ms - a.created_at_ms)
      .map(jobFromNative);
  }
  async function readGpuMemory() {
    try {
      state.gpuMemory = await call<GpuMemoryStatus>("get_stt_memory_status");
      state.gpuMemoryError = "";
    } catch {
      state.gpuMemory = undefined;
      state.gpuMemoryError =
        "Не удалось получить состояние видеопамяти. Проверка повторится автоматически.";
    }
  }
  async function refresh() {
    await readModels();
    await readSettings();
    const results = await Promise.allSettled([
      readHistory(),
      readDevices(),
      readService(),
      readGpuMemory(),
      call<{ cuda: boolean; vulkan: boolean }>(
        "get_acceleration_capabilities",
      ).then((c) => {
        state.accelerations = [
          { value: "auto", label: "Авто · рекомендуется" },
          { value: "cpu", label: "Процессор" },
          ...(c.cuda ? [{ value: "cuda", label: "NVIDIA CUDA" }] : []),
          ...(c.vulkan ? [{ value: "vulkan", label: "Vulkan" }] : []),
        ];
      }),
      call<WakeWordCapabilities>("get_wake_word_capabilities").then((c) => {
        state.wakePhrases = c.supported_phrases;
      }),
      call<string>("get_wake_word_status").then((s) => {
        state.wakeStatus = s;
      }),
      call<string>("get_recent_logs", { lines: 80 }).then((log) => {
        state.logs = log.split("\n").reverse();
      }),
    ]);
    for (const result of results)
      if (result.status === "rejected") report(result.reason);
  }
  function saveRaw(change: (settings: Settings) => Settings) {
    return serialize(async () => {
      const current = await call<Settings>("get_settings");
      await call("save_settings", { settings: change(current) });
      await readSettings();
    });
  }
  async function save(patch: Partial<Preferences>) {
    await saveRaw((current) => applyPreferences(current, patch, models));
    await readDevices();
    if (patch.overlayPosition && patch.overlayPosition !== "custom")
      await call("position_overlay", { position: patch.overlayPosition });
    await readHistory();
    await readGpuMemory();
  }
  return {
    state,
    serialize,
    report,
    refresh,
    save,
    saveRaw,
    readSettings,
    readModels,
    readHistory,
    readDevices,
    readService,
    readGpuMemory,
  };
}
export type NativeContext = ReturnType<typeof createNativeContext>;
