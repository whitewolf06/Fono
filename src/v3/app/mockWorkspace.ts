import { reactive } from "vue";
import type {
  Workspace,
  WorkspaceState,
  Scenario,
  LiveDictation,
} from "../shared/domain/contracts";
import { defaults } from "../features/preferences/domain/preferences";
import { createSettingsPort } from "../features/preferences/infrastructure/mockSettings";
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
    logs: [
      "Демонстрационный режим готов. Системные устройства не используются.",
    ],
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
        active.text =
          "Демонстрационная задача выполнена. Голос превратился в текст.";
        jobTicks = 0;
      }
    } else {
      const next = state.jobs.find((j) => j.state === "queued");
      if (next) next.state = "running";
    }
  }, 800);
  function scenario(value: Scenario) {
    dictation.cancel();
    state.preferences = { ...defaults };
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
      state.preferences.processingModel =
        "Локальная модель с очень длинным названием для проверки компоновки и переносов";
    }
    selectLatest(state, state.history[0] ?? null);
    if (value.startsWith("live-")) {
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
          live.warning =
            "Вставка не подтверждена. Текст сохранён; проверьте выбранное поле перед продолжением.";
        }
      }
    }
  }
  return {
    state,
    wake,
    dictation,
    settings: createSettingsPort(state),
    commands: createCommandsPort(state),
    service: createServicePort(state),
    history: {
      remove(id) {
        state.history = state.history.filter((e) => e.id !== id);
      },
      clear() {
        state.history = [];
      },
    },
    trainer: {
      clear() {
        state.history = state.history.map(
          ({ original: _original, ...entry }) => entry,
        );
      },
    },
    copy: copyToClipboard,
    scenario,
    diagnostics() {
      const model = state.models.find((m) => m.id === state.preferences.model);
      return [
        {
          id: "mic",
          title: "Микрофон",
          health: state.microphoneAvailable ? "ready" : "missing",
          detail: state.microphoneAvailable
            ? "Устройство доступно (демо)"
            : "Подключите микрофон и выберите его в настройках",
          section: "audio",
        },
        {
          id: "model",
          title: "Распознавание",
          health:
            model?.status === "installed"
              ? "ready"
              : model?.status === "downloading"
                ? "loading"
                : "missing",
          detail:
            model?.status === "installed"
              ? model.name + " готова"
              : "Загрузите или выберите установленную модель",
          section: "audio",
        },
        {
          id: "ai",
          title: "Обработка текста",
          health: !state.preferences.processingEnabled
            ? "off"
            : state.aiAvailable
              ? "ready"
              : "error",
          detail: state.aiAvailable
            ? "Подключение доступно (демо)"
            : "Проверьте адрес сервера и доступность модели",
          section: "processing",
        },
        {
          id: "service",
          title: "API-сервис",
          health: state.preferences.serviceEnabled ? "ready" : "off",
          detail: "127.0.0.1:17832 · демонстрационное состояние",
          section: "diagnostics",
        },
      ];
    },
    resumeOnboarding(step) {
      state.onboardingStep = step;
      saveWizardStep(step);
    },
    dispose() {
      disposed = true;
      clearInterval(jobsTimer);
      dictation.dispose();
    },
  };
}
