import { message as uiMessage } from "../shared/domain/messages";
import { createNativeHistoryRewrite } from "../features/history/infrastructure/nativeRewrite";
import { createNativeSnippetTransfer } from "../features/snippets/infrastructure/nativeSnippets";
import { createNativeFiles } from "../features/files/infrastructure/nativeFiles";
import { reactive } from "vue";
import type { Workspace, WorkspaceState } from "../shared/domain/contracts";
import { defaults } from "../features/preferences/domain/preferences";
import { createNativeContext } from "../shared/infrastructure/native/context";
import { nativeSettings } from "../shared/infrastructure/native/settings";
import { createTriggerCapture } from "../shared/infrastructure/triggerCapture";
import { createNativeSelectionCapture } from "../features/selection/infrastructure/nativeSelection";
import {
  nativeDictation,
  type NativeResult,
  type NativeLiveSnapshot,
} from "../shared/infrastructure/native/dictation";
import {
  call,
  subscribe,
  subscribeBackend,
} from "../shared/infrastructure/native/ipc";
import { nativeWake } from "../shared/infrastructure/native/wake";
import { createNativeWakeSamples } from "../features/wake-samples/infrastructure/nativeSamples";
import { nativeCommands } from "../features/commands/infrastructure/nativeCommands";
import { createNativeFeedbackChannel } from "../features/feedback";
import { createNativeDiagnosticReport } from "../features/diagnostic-report";
import { createNativeUpdates } from "../features/updates";
import { createNativeCudaPackages } from "../features/cuda-packages/infrastructure/nativeCudaPackages";
import type { PendingDictation } from "../shared/domain/processing";
import { createNativeTrainer } from "../features/trainer/infrastructure/nativeTrainer";
import { bindNativeVisibility } from "../shared/infrastructure/native/visibility";
import { createCoalescedRead } from "../shared/infrastructure/native/coalescedRead";
import type { BackendEventV1 } from "../shared/infrastructure/native/backendEvents";
type NativeSttReadiness = Extract<
  BackendEventV1,
  { kind: "stt_readiness" }
>["payload"]["readiness"];
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
  const visibility = bindNativeVisibility(() => void refresh());
  const ctx = createNativeContext(state, visibility.visible);
  const updates = createNativeUpdates(
    __FONO_FRONTEND_BUILD__.version,
    call,
    (action) =>
      ctx.serialize(async () => {
        await action();
        await ctx.readSettings();
      }),
    visibility.visible,
  );
  const dictation = nativeDictation(ctx);
  const cudaPackages = createNativeCudaPackages(
    call,
    ctx.refresh,
    visibility.visible,
  );
  const trainer = createNativeTrainer(call, ctx.readHistory);
  const selection = createNativeSelectionCapture(
    call,
    subscribe,
    visibility.visible,
  );
  const wake = nativeWake(ctx);
  const wakeSamples = createNativeWakeSamples(call);
  let disposed = false;
  const sessionRead = createCoalescedRead(
    () => dictation.refresh(),
    () => {},
    () => !disposed && visibility.visible(),
  );
  let sttError = "";
  function applySttReadiness(readiness: NativeSttReadiness) {
    state.sttReadiness = readiness;
    if (readiness.state === "failed") {
      sttError = readiness.message;
      ctx.report(sttError);
    } else if (readiness.state === "ready") {
      if (sttError && state.error === sttError) state.error = "";
      sttError = "";
    }
  }
  const sttReadiness = createCoalescedRead(
    () => call<NativeSttReadiness>("get_stt_readiness"),
    applySttReadiness,
    () => !disposed && visibility.visible(),
  );
  const releases: (() => void)[] = [];
  const subscriptions: Promise<void>[] = [];
  function bind<T>(channel: string, handler: (value: T) => void) {
    subscriptions.push(
      subscribe<T>(channel, (value) => {
        if (!disposed) handler(value);
      })
        .then((release) => {
          if (disposed) release();
          else releases.push(release);
        })
        .catch(ctx.report),
    );
  }
  bind<NativeResult>("dictation-result", dictation.accept);
  bind<PendingDictation | null>("pending-dictation", (value) => {
    if (visibility.visible()) dictation.pending(value);
  });
  if (
    typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
    __FONO_EXPERIMENTAL_LIVE__
  )
    bind<NativeLiveSnapshot>("dictation-live", dictation.live);
  bind<string>("command-proposal", (proposal) => {
    state.commandProposal = proposal;
  });
  if (typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__)
    bind<{ reason: string; message: string }>(
      "wake-word-configuration-invalidated",
      (event) => {
        state.logs.unshift(event.message);
        void wake.load().catch(ctx.report);
      },
    );
  bind(
    "speech-analysis-changed",
    () => void ctx.readHistory().catch(ctx.report),
  );
  if (typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__)
    bind<string>("wake-word-status", (status) => {
      state.wakeStatus = status;
    });
  if (typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__)
    bind<{ remaining_ms: number; timeout_ms: number; speaking: boolean }>(
      "wake-dictation-countdown",
      (countdown) => {
        state.countdown = countdown;
      },
    );
  subscriptions.push(
    subscribeBackend((event) => {
      if (disposed || !visibility.visible()) return;
      switch (event.kind) {
        case "dictation_outcome":
          dictation.outcome(event.payload);
          void sessionRead.refresh().catch(ctx.report);
          return;
        case "audio_level":
          dictation.level(event.payload);
          return;
        case "pipeline_state":
        case "pipeline_mode":
          void sessionRead.refresh().catch(ctx.report);
          return;
        case "settings":
          void ctx.readSettings().catch(ctx.report);
          return;
        case "history_changed":
          void ctx.readHistory().catch(ctx.report);
          return;
        case "service":
          void ctx.readService().catch(ctx.report);
          return;
        case "gpu_memory_changed":
          ctx.acceptGpuMemory(event.payload);
          return;
        case "stt_readiness":
          sttReadiness.invalidate();
          applySttReadiness(event.payload.readiness);
          return;
        case "error":
          ctx.report(event.payload.message);
          void sessionRead.refresh().catch(ctx.report);
          return;
      }
      if (event.kind !== "model_download") return;
      const progress = event.payload;
      const model = state.models.find(
        (m) =>
          progress.download_id ===
          (m.id.includes(":") ? m.id : "whisper:" + m.id),
      );
      if (
        model &&
        !["completed", "cancelled", "failed"].includes(progress.phase)
      ) {
        model.status = "downloading";
        model.downloadPhase =
          progress.phase === "verifying" ? "verifying" : "downloading";
        model.progress = progress.total_bytes
          ? Math.min(
              99,
              Math.round(
                (100 * progress.downloaded_bytes) / progress.total_bytes,
              ),
            )
          : 0;
      } else {
        if (model) {
          model.status = "available";
          if (progress.phase === "failed")
            model.downloadError = uiMessage("ui.a5d23df4cf");
        }
        void ctx.readModels().catch(ctx.report);
      }
    })
      .then((release) => {
        if (disposed) release();
        else releases.push(release);
      })
      .catch(ctx.report),
  );
  async function refresh() {
    try {
      await Promise.all(subscriptions);
      if (disposed || !visibility.visible()) return;
      await Promise.all([
        ctx.refresh(),
        sttReadiness.refresh(),
        selection.refresh?.(),
        wakeSamples.refreshLive(),
      ]);
      if (disposed || !visibility.visible()) return;
      await sessionRead.refresh();
      await wake.load();
      await updates.refresh();
      await cudaPackages.refresh();
      state.scenario = "normal";
    } catch (error) {
      ctx.report(error);
      state.scenario = "load-error";
    }
  }
  void refresh();
  return {
    native: true,
    report: createNativeDiagnosticReport(),
    feedback: createNativeFeedbackChannel(),
    updates,
    cudaPackages,
    state,
    refresh,
    wake,
    wakeSamples,
    settings: nativeSettings(ctx),
    selection,
    triggerCapture: createTriggerCapture(state, {
      begin: () => call<string>("begin_trigger_capture"),
      end: (id) => call<void>("end_trigger_capture", { id }),
    }),
    dictation: dictation.port,
    history: {
      async clearApplicationContext(id) {
        await call("clear_application_context", { id: id ?? null });
        await ctx.readHistory();
      },
      async setIncluded(id, included) {
        const found = await call<boolean>(
          "set_dictation_history_entry_analytics_included",
          { id, included },
        );
        if (!found) throw new Error(uiMessage("ui.a0a69d42bc"));
        await ctx.readHistory();
      },
      async remove(id) {
        await call("delete_dictation_history_entry", { id });
        await ctx.readHistory();
      },
      async clear() {
        await call("clear_dictation_history");
        await ctx.readHistory();
      },
    },
    historyRewrite: createNativeHistoryRewrite(call),
    trainer,
    commands: nativeCommands(ctx),
    files: createNativeFiles(call, ctx.readService),
    snippetTransfer: createNativeSnippetTransfer(call),
    service: {
      async cancel(id) {
        await call("cancel_local_transcription_job", { id });
        await ctx.readService();
      },
      enqueue() {
        throw new Error(uiMessage("ui.1185988b25"));
      },
      retry() {
        throw new Error(uiMessage("ui.647eba583c"));
      },
      async copyToken() {
        await call("copy_local_transcription_api_token");
      },
    },
    copy: (text) => call("copy_dictation_text", { text }),
    openProjectSite: () => call("open_project_site"),
    scenario() {
      void refresh();
    },
    diagnostics() {
      return [
        {
          id: "mic",
          get title() {
            return uiMessage("ui.f038415295");
          },
          health: state.microphoneAvailable ? "ready" : "missing",
          detail: state.microphoneAvailable
            ? uiMessage("ui.111bb55c3d")
            : uiMessage("ui.36675b2e6f"),
          section: "audio",
        },
        {
          id: "model",
          get title() {
            return uiMessage("ui.7894eb1ac6");
          },
          health: state.models.some(
            (m) => m.id === state.preferences.model && m.status === "installed",
          )
            ? "ready"
            : "missing",
          detail: state.preferences.model || uiMessage("ui.c4dac53b9a"),
          section: "audio",
        },
        {
          id: "ai",
          get title() {
            return uiMessage("ui.d4a26d3502");
          },
          health: !state.preferences.processingEnabled
            ? "off"
            : state.aiAvailable
              ? "ready"
              : "missing",
          detail: state.aiAvailable
            ? uiMessage("ui.a31fc40f3a")
            : uiMessage("ui.7d89971863"),
          section: "processing",
        },
        {
          id: "service",
          get title() {
            return uiMessage("ui.1b580ef9f9");
          },
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
      wakeSamples.dispose();
      selection.dispose();
      disposed = true;
      sessionRead.dispose();
      sttReadiness.dispose();
      ctx.dispose();
      dictation.dispose();
      visibility.dispose();
      void trainer.cancelPeriod().catch(ctx.report);
      updates.dispose();
      cudaPackages.dispose();
      releases.forEach((release) => release());
    },
  };
}
