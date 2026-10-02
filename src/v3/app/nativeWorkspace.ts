import { reactive } from "vue";
import type { Workspace, WorkspaceState } from "../shared/domain/contracts";
import { defaults } from "../features/preferences/domain/preferences";
import { createNativeContext } from "../shared/infrastructure/native/context";
import { nativeSettings } from "../shared/infrastructure/native/settings";
import {
  nativeDictation,
  type NativeSnapshot,
  type NativeResult,
} from "../shared/infrastructure/native/dictation";
import { call, subscribe } from "../shared/infrastructure/native/ipc";
import { nativeWake } from "../shared/infrastructure/native/wake";
import { nativeCommands } from "../features/commands/infrastructure/nativeCommands";
export function createNativeWorkspace(): Workspace {
  const state = reactive<WorkspaceState>({
    preferences: {
      ...defaults,
      wakeEnabled: false,
      processingEnabled: false,
      serviceEnabled: false,
    },
    profiles: [],
    models: [],
    history: [],
    applications: [],
    jobs: [],
    last: {
      entry: null,
      variant: "result",
      draft: "",
      edited: false,
      undo: null,
    },
    phase: "idle",
    audioLevel: 0,
    elapsed: 0,
    error: "",
    pending: {},
    scenario: "loading",
    microphoneAvailable: false,
    aiAvailable: false,
    logs: [],
    onboardingStep: 0,
    testSignal: 0,
  });
  const ctx = createNativeContext(state);
  const dictation = nativeDictation(ctx);
  let disposed = false;
  let polling = false;
  const releases: (() => void)[] = [];
  function bind<T>(channel: string, handler: (value: T) => void) {
    void subscribe(channel, handler)
      .then((release) => (disposed ? release() : releases.push(release)))
      .catch(ctx.report);
  }
  bind<NativeResult>("dictation-result", dictation.accept);
  bind<string>("command-proposal", (proposal) => {
    state.commandProposal = proposal;
  });
  bind("settings-changed", () => void ctx.readSettings().catch(ctx.report));
  bind(
    "speech-analysis-changed",
    () => void ctx.readHistory().catch(ctx.report),
  );
  bind(
    "local-transcription-service-changed",
    () => void ctx.readService().catch(ctx.report),
  );
  bind<string>("error", (message) => ctx.report(message));
  bind<string>("wake-word-status", (status) => {
    state.wakeStatus = status;
  });
  bind<{ remaining_ms: number; timeout_ms: number; speaking: boolean }>(
    "wake-dictation-countdown",
    (countdown) => {
      state.countdown = countdown;
    },
  );
  bind<{
    kind: string;
    payload: {
      model: string;
      download_id: string;
      phase: string;
      downloaded_bytes: number;
      total_bytes?: number;
    };
  }>("backend-event-v1", (event) => {
    if (event.kind !== "model_download") return;
    const progress = event.payload;
    const model = state.models.find(
      (m) => progress.download_id === "whisper:" + m.id,
    );
    if (model && !["completed", "cancelled"].includes(progress.phase)) {
      model.status = "downloading";
      model.progress = progress.total_bytes
        ? Math.min(
            99,
            Math.round(
              (100 * progress.downloaded_bytes) / progress.total_bytes,
            ),
          )
        : 0;
    } else {
      if (model) model.status = "available";
      void ctx.readModels().catch(ctx.report);
    }
  });
  async function refresh() {
    try {
      await ctx.refresh();
      state.scenario = "normal";
    } catch (error) {
      ctx.report(error);
      state.scenario = "load-error";
    }
  }
  void refresh();
  const timer = setInterval(async () => {
    if (disposed || polling || state.scenario !== "normal") return;
    polling = true;
    try {
      dictation.snapshot(await call<NativeSnapshot>("get_desktop_snapshot"));
    } catch (error) {
      ctx.report(error);
    } finally {
      polling = false;
    }
  }, 160);
  const dataTimer = setInterval(() => {
    if (!disposed && state.scenario === "normal")
      void Promise.all([ctx.readService(), ctx.readHistory()]).catch(
        ctx.report,
      );
  }, 3000);
  return {
    native: true,
    state,
    refresh,
    wake: nativeWake(ctx),
    settings: nativeSettings(ctx),
    dictation: dictation.port,
    history: {
      async remove(id) {
        await call("delete_dictation_history_entry", { id });
        await ctx.readHistory();
      },
      async clear() {
        await call("clear_dictation_history");
        await ctx.readHistory();
      },
    },
    trainer: {
      async clear() {
        await call("clear_speech_analytics");
        await ctx.readHistory();
      },
      async recommend(id) {
        const result = await call<{
          summary: string;
          recommendations: { title: string; exercise: string }[];
        }>("recommend_speech", { id });
        return [
          result.summary,
          ...result.recommendations.map((r) => r.title + ": " + r.exercise),
        ].join("\n\n");
      },
    },
    commands: nativeCommands(ctx),
    service: {
      async cancel(id) {
        await call("cancel_local_transcription_job", { id });
        await ctx.readService();
      },
      enqueue() {
        throw new Error("Отправьте аудиофайл через API: вкладка «Подключение»");
      },
      retry() {
        throw new Error("Повторно отправьте исходный аудиофайл через API");
      },
      async copyToken() {
        await call("copy_local_transcription_api_token");
      },
    },
    copy: (text) => call("copy_dictation_text", { text }),
    scenario() {
      void refresh();
    },
    diagnostics() {
      return [
        {
          id: "mic",
          title: "Микрофон",
          health: state.microphoneAvailable ? "ready" : "missing",
          detail: state.microphoneAvailable
            ? "Устройство доступно"
            : "Подключите микрофон",
          section: "audio",
        },
        {
          id: "model",
          title: "Распознавание",
          health: state.models.some(
            (m) => m.id === state.preferences.model && m.status === "installed",
          )
            ? "ready"
            : "missing",
          detail: state.preferences.model || "Выберите модель",
          section: "audio",
        },
        {
          id: "ai",
          title: "Обработка текста",
          health: !state.preferences.processingEnabled
            ? "off"
            : state.aiAvailable
              ? "ready"
              : "missing",
          detail: state.aiAvailable
            ? "Подключение проверено"
            : "Проверьте подключение в настройках",
          section: "processing",
        },
        {
          id: "service",
          title: "API-сервис",
          health: state.serviceError
            ? "error"
            : state.preferences.serviceEnabled
              ? "ready"
              : "off",
          detail: state.serviceError || state.serviceAddress || "",
          section: "diagnostics",
        },
      ];
    },
    resumeOnboarding(step) {
      state.onboardingStep = step;
    },
    dispose() {
      disposed = true;
      clearInterval(timer);
      clearInterval(dataTimer);
      releases.forEach((release) => release());
    },
  };
}
