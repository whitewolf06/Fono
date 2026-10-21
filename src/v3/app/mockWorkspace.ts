import { message as uiMessage } from "../shared/domain/messages";
import { createBrowserSnippetTransfer } from "../features/snippets/infrastructure/browserSnippets";
import { createMockFiles } from "../features/files/infrastructure/mockFiles";
import { reactive, watch } from "vue";
import { createMockHistoryRewrite } from "../features/history/infrastructure/mockRewrite";
import { openProjectSite } from "../shared/infrastructure/browser";
import { createBrowserFeedbackChannel } from "../features/feedback";
import { createMockDiagnosticReport } from "../features/diagnostic-report";
import { createMockUpdates } from "../features/updates";
import { createMockCudaPackages } from "../features/cuda-packages/infrastructure/mockCudaPackages";
import { LIVE_DICTATION_ENABLED } from "../shared/domain/dictationMode";
import { createMockTrainer } from "../features/trainer/infrastructure/mockTrainer";
import type {
  Workspace,
  WorkspaceState,
  Scenario,
  LiveDictation,
} from "../shared/domain/contracts";
import { defaults } from "../features/preferences/domain/preferences";
import { clonePreferences } from "../features/preferences/domain/processingPrompts";
import { createSettingsPort } from "../features/preferences/infrastructure/mockSettings";
import { createTriggerCapture } from "../shared/infrastructure/triggerCapture";
import { createMockSelectionCapture } from "../features/selection/infrastructure/mockSelection";
import { createMockWake } from "../features/preferences/infrastructure/mockWake";
import {
  createDictationPort,
  selectLatest,
} from "../features/dictation/infrastructure/mockDictation";
import { createCommandsPort } from "../features/commands/infrastructure/mockCommands";
import { createServicePort } from "../features/service/infrastructure/mockService";
import {
  demoHistory,
  demoModels,
  demoJobs,
  demoApps,
} from "../shared/infrastructure/fixtures";
import {
  readPreferences,
  copyToClipboard,
  readWizardStep,
  saveWizardStep,
} from "../shared/infrastructure/browser";
export function createMockWorkspace(): Workspace {
  const profileDefaults = () => [
    {
      id: "local",
      name: "На компьютере · LM Studio",
      provider: "lmstudio",
      location: "local",
      url: "http://127.0.0.1:1234/v1",
      model: "Qwen 3 · 8B",
    },
    {
      id: "cloud",
      name: "В облаке · OpenAI (демо)",
      provider: "openai",
      location: "cloud",
      url: "https://api.openai.com/v1",
      model: "GPT-4o Mini",
    },
  ];
  const state = reactive<WorkspaceState>({
    profiles: profileDefaults(),
    preferences: readPreferences(),
    history: demoHistory(),
    models: demoModels(),
    applications: demoApps(),
    jobs: demoJobs(),
    last: {
      entry: null,
      variant: "original",
      draft: "",
      edited: false,
      undo: null,
    },
    phase: "idle",
    audioLevel: 0,
    elapsed: 0,
    error: "",
    pending: {},
    scenario: "normal",
    microphoneAvailable: true,
    aiAvailable: true,
    logs: [uiMessage("ui.a196172e2b")],
    onboardingStep: readWizardStep(),
    testSignal: 0,
  });
  const selectedModel = state.models.find(
    (m) => m.id === state.preferences.model,
  );
  if (selectedModel) {
    selectedModel.status = "installed";
    selectedModel.progress = 100;
  }
  selectLatest(state, state.history[0]);
  const dictation = createDictationPort(state);
  const trainer = createMockTrainer(state);
  const historyRewrite = createMockHistoryRewrite(state);
  const stopRewritePrivacy = watch(
    () => [
      state.preferences.analyticsConsent,
      ...state.history.map((e) => [e.id, e.original]),
    ],
    historyRewrite.prune,
    { deep: true, flush: "sync" },
  );
  const selection = createMockSelectionCapture({
    preferences: () => state.preferences,
    accepted: (text) =>
      state.history.unshift({
        id: `selection-demo-${Date.now()}`,
        text,
        title: text.slice(0, 64),
        createdAt: new Date().toISOString(),
        duration: 0,
        analyticsIncluded: false,
        findings: [],
      }),
  });
  const updates = createMockUpdates(__FONO_FRONTEND_BUILD__.version);
  const cudaPackages = createMockCudaPackages();
  const wake = createMockWake(state);
  let disposed = false;
  let jobTicks = 0;
  const jobsTimer = setInterval(() => {
    if (
      disposed ||
      !state.preferences.serviceEnabled ||
      state.scenario === "queue"
    )
      return;
    const active = state.jobs.find((j) => j.state === "running");
    if (active) {
      jobTicks++;
      if (jobTicks >= 3) {
        active.state = "done";
        active.text = uiMessage("ui.1aa8b24ef5");
        jobTicks = 0;
      }
    } else {
      const next = state.jobs.find((j) => j.state === "queued");
      if (next) next.state = "running";
    }
  }, 800);
  function scenario(value: Scenario) {
    trainer.reset();
    dictation.cancel();
    state.preferences = clonePreferences(defaults);
    state.history = demoHistory();
    state.models = demoModels();
    state.profiles = profileDefaults();
    state.jobs = demoJobs();
    state.microphoneAvailable = true;
    state.aiAvailable = true;
    state.phase = "idle";
    state.live = null;
    state.error = "";
    state.scenario = value;
    wake.resetDemo();
    state.pending = {};
    if (value === "empty") {
      state.history = [];
      state.jobs = [];
      state.applications = [];
    }
    if (value === "no-microphone") state.microphoneAvailable = false;
    if (value === "no-model")
      state.models.forEach((m) => {
        m.status = "available";
        m.progress = 0;
      });
    if (value === "download") {
      state.models[1].status = "downloading";
      state.models[1].progress = 46;
    }
    if (value === "ai-error") state.aiAvailable = false;
    if (value === "service-off") state.preferences.serviceEnabled = false;
    if (value === "queue")
      state.jobs = [1, 2, 3, 4].map((i) => ({
        id: "queue-" + i,
        name: "Запись встречи " + i + ".wav",
        state: i === 1 ? "running" : "queued",
        seconds: 120,
      }));
    if (value === "long-content") {
      state.history[0].original =
        "Большая диктовка: мысли, вопросы и подробные наблюдения для проверки интерфейса. ".repeat(
          100,
        );
      state.preferences.processingModel = uiMessage("ui.f8597fe4d5");
    }
    selectLatest(state, state.history[0] ?? null);
    if (LIVE_DICTATION_ENABLED && value.startsWith("live-")) {
      state.preferences.dictationMode = "live";
      dictation.start();
      const live = state.live as LiveDictation | null;
      if (live) {
        live.committedText = "Да, да, это уже подтверждённый текст.";
        live.draftText = "Следующая мысль ещё уточняется";
        if (value === "live-paused") live.insertionState = "paused_focus";
        if (value === "live-backlog") live.lagMs = 8500;
        if (value === "live-insertion-error") {
          live.insertionState = "failed";
          live.warning = uiMessage("ui.89b932fb8f");
        }
      }
    }
  }
  return {
    report: createMockDiagnosticReport(state),
    feedback: createBrowserFeedbackChannel(),
    updates,
    cudaPackages,
    state,
    wake,
    dictation,
    settings: createSettingsPort(state),
    selection,
    triggerCapture: createTriggerCapture(state),
    commands: createCommandsPort(state),
    service: createServicePort(state),
    files: createMockFiles(state),
    snippetTransfer: createBrowserSnippetTransfer(),
    history: {
      async setIncluded(id, included) {
        const entry = state.history.find((item) => item.id === id);
        if (!entry) throw new Error(uiMessage("ui.a0a69d42bc"));
        entry.analyticsIncluded = included;
        if (!included && entry.metadata) delete entry.metadata.speechTiming;
        trainer.invalidate();
      },
      remove(id) {
        state.history = state.history.filter((e) => e.id !== id);
        historyRewrite.prune();
        trainer.invalidate();
      },
      clear() {
        state.history = [];
        historyRewrite.clear();
        trainer.invalidate();
      },
    },
    historyRewrite: historyRewrite.port,
    trainer: trainer.port,
    copy: copyToClipboard,
    openProjectSite,
    scenario,
    diagnostics() {
      const model = state.models.find((m) => m.id === state.preferences.model);
      return [
        {
          id: "mic",
          title: uiMessage("ui.f038415295"),
          health: state.microphoneAvailable ? "ready" : "missing",
          detail: state.microphoneAvailable
            ? uiMessage("ui.d055cd8a83")
            : uiMessage("ui.b61bd4e1f3"),
          section: "audio",
        },
        {
          id: "model",
          title: uiMessage("ui.7894eb1ac6"),
          health:
            model?.status === "installed"
              ? "ready"
              : model?.status === "downloading"
                ? "loading"
                : "missing",
          detail:
            model?.status === "installed"
              ? model.name + uiMessage("ui.442325a3bb")
              : uiMessage("ui.5c9cd7bd23"),
          section: "audio",
        },
        {
          id: "ai",
          title: uiMessage("ui.d4a26d3502"),
          health: !state.preferences.processingEnabled
            ? "off"
            : state.aiAvailable
              ? "ready"
              : "error",
          detail: state.aiAvailable
            ? uiMessage("ui.be66732e94")
            : uiMessage("ui.da02c784d3"),
          section: "processing",
        },
        {
          id: "service",
          title: uiMessage("ui.1b580ef9f9"),
          health: state.preferences.serviceEnabled ? "ready" : "off",
          get detail() {
            return uiMessage("ui.93169fcc97");
          },
          section: "diagnostics",
        },
      ];
    },
    resumeOnboarding(step) {
      state.onboardingStep = step;
      saveWizardStep(step);
    },
    dispose() {
      stopRewritePrivacy();
      historyRewrite.dispose();
      selection.dispose();
      disposed = true;
      trainer.dispose();
      updates.dispose();
      cudaPackages.dispose();
      clearInterval(jobsTimer);
      dictation.dispose();
    },
  };
}
