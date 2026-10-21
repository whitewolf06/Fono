import { message as uiMessage } from "../../domain/messages";
import type { WorkspaceState, Preferences } from "../../domain/contracts";
import type {
  Settings,
  SttModelInfo,
  DeviceInfo,
  WakeWordCapabilities,
  LocalTranscriptionServiceSnapshot,
} from "./ipcTypes";
import { call } from "./ipc";
import { createCoalescedRead } from "./coalescedRead";
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
export function createNativeContext(
  state: WorkspaceState,
  visible: () => boolean = () => true,
) {
  let models: SttModelInfo[] = [];
  let saveTail: Promise<unknown> = Promise.resolve();
  let historyInitialized = false;
  let disposed = false;
  let settingsGeneration = 0;
  let refreshGeneration = 0;
  let devicesGeneration = 0;
  const active = () => !disposed && visible();
  function serialize<T>(action: () => Promise<T>): Promise<T> {
    const task = saveTail.catch(() => {}).then(action);
    saveTail = task;
    return task;
  }
  function report(error: unknown) {
    if (disposed) return;
    const message = error instanceof Error ? error.message : String(error);
    state.error = message;
    state.logs.unshift(message);
  }
  async function readSettings() {
    const ticket = ++settingsGeneration;
    const raw = await call<Settings>("get_settings");
    if (!active() || ticket !== settingsGeneration) return raw;
    const custom =
      raw.whisper_model_path &&
      !models.some((model) => model.local_path === raw.whisper_model_path);
    const selected = custom
      ? await call<{ path: string; filename: string; bytes: number } | null>(
          "get_selected_model_metadata",
        )
      : null;
    // Settings callers, including wake research, retain their Settings return value;
    // only the latest live request may publish preferences or custom-model metadata.
    if (!active() || ticket !== settingsGeneration) return raw;
    state.preferences = preferencesFromNative(raw, models);
    state.profiles = profilesFromNative(raw);
    if (custom) {
      state.models = state.models.filter((model) => !model.custom);
      if (selected && selected.path === raw.whisper_model_path)
        state.models.push({
          id: selected.path,
          name: selected.filename,
          size:
            Math.round(selected.bytes / 1048576) + uiMessage("ui.883817f199"),
          get description() {
            return uiMessage("ui.9af64c0d6f");
          },
          status: "installed",
          progress: 100,
          custom: true,
        });
    }
    state.applications = raw.launch_apps.map((app, index) => ({
      id: String(index),
      name: app.name,
      path: app.exe_path,
      phrase: app.aliases.join(", "),
    }));
    return raw;
  }
  const modelRead = createCoalescedRead(
    () => call<SttModelInfo[]>("list_stt_models"),
    (value) => {
      models = value;
      const downloading = state.models.filter(
        (model) => model.status === "downloading",
      );
      const errors = new Map(
        state.models
          .filter((m) => m.downloadError)
          .map((m) => [m.id, m.downloadError]),
      );
      state.models = modelsFromNative(models).map((model) =>
        model.status === "installed"
          ? model
          : downloading.find((download) => download.id === model.id) || model,
      );
      state.models.forEach((m) => {
        if (errors.has(m.id)) m.downloadError = errors.get(m.id);
      });
    },
    active,
  );
  const readModels = modelRead.refresh;
  const historyRead = createCoalescedRead(
    () => call<DictationHistoryEntry[]>("get_dictation_history"),
    (value) => {
      state.history = value.map(historyFromNative);
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
    },
    active,
  );
  const readHistory = historyRead.refresh;
  async function readDevices() {
    if (!active()) return;
    const ticket = ++devicesGeneration;
    const devices = await call<DeviceInfo[]>("list_audio_devices");
    if (!active() || ticket !== devicesGeneration) return;
    state.devices = [
      {
        value: "system",
        get label() {
          return uiMessage("ui.e32ad3f607");
        },
      },
      ...devices.map((d) => ({ value: d.id, label: d.name })),
    ];
    state.microphoneAvailable = devices.some((d) =>
      state.preferences.microphone === "system"
        ? d.is_default
        : d.id === state.preferences.microphone,
    );
  }
  const serviceRead = createCoalescedRead(
    () =>
      call<
        LocalTranscriptionServiceSnapshot & {
          enabled: boolean;
          error: string | null;
        }
      >("get_local_transcription_service_snapshot"),
    (service) => {
      state.serviceAddress = service.address;
      state.serviceError = service.error || "";
      if (!state.pending.serviceEnabled)
        state.preferences.serviceEnabled = service.enabled;
      state.jobs = [
        ...new Map(
          [...service.history.jobs, ...service.queue.jobs].map((job) => [
            job.id,
            job,
          ]),
        ).values(),
      ]
        .sort((left, right) => right.created_at_ms - left.created_at_ms)
        .map(jobFromNative);
    },
    active,
  );
  const readService = serviceRead.refresh;
  const gpuRead = createCoalescedRead(
    async () => {
      try {
        return { status: await call<GpuMemoryStatus>("get_stt_memory_status") };
      } catch {
        return { status: undefined };
      }
    },
    ({ status }) => {
      state.gpuMemory = status;
      state.gpuMemoryError = status ? "" : uiMessage("ui.b72ef9d00c");
    },
    active,
  );
  const readGpuMemory = gpuRead.refresh;
  function acceptGpuMemory(status: GpuMemoryStatus) {
    gpuRead.invalidate();
    if (!active()) return;
    state.gpuMemory = status;
    state.gpuMemoryError = "";
  }
  async function refresh() {
    const ticket = ++refreshGeneration;
    const current = () => active() && ticket === refreshGeneration;
    if (!current()) return;
    await readModels();
    if (!current()) return;
    await readSettings();
    if (!current()) return;
    const results = await Promise.allSettled([
      readHistory(),
      readDevices(),
      readService(),
      readGpuMemory(),
      call<{ cuda: boolean; vulkan: boolean }>(
        "get_acceleration_capabilities",
      ).then((c) => {
        if (!current()) return;
        state.accelerations = [
          {
            value: "auto",
            get label() {
              return uiMessage("ui.0cdf37c953");
            },
          },
          {
            value: "cpu",
            get label() {
              return uiMessage("ui.df6652413d");
            },
          },
          ...(c.cuda ? [{ value: "cuda", label: "NVIDIA CUDA" }] : []),
          ...(c.vulkan ? [{ value: "vulkan", label: "Vulkan" }] : []),
        ];
      }),
      call<WakeWordCapabilities>("get_wake_word_capabilities").then((c) => {
        if (!current()) return;
        state.wakePhrases = c.supported_phrases;
      }),
      call<string>("get_wake_word_status").then((s) => {
        if (!current()) return;
        state.wakeStatus = s;
      }),
      call<string>("get_recent_logs", { lines: 80 }).then((log) => {
        if (!current()) return;
        state.logs = log.split("\n").reverse();
      }),
    ]);
    if (!current()) return;
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
    acceptGpuMemory,
    dispose() {
      disposed = true;
      settingsGeneration++;
      devicesGeneration++;
      refreshGeneration++;
      [modelRead, historyRead, serviceRead, gpuRead].forEach((reader) =>
        reader.dispose(),
      );
    },
  };
}
export type NativeContext = ReturnType<typeof createNativeContext>;
