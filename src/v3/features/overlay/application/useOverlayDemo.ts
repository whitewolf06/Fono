import { ref, onScopeDispose } from "vue";
import type {
  Phase,
  LiveDictation,
  Preferences,
} from "../../../shared/domain/contracts";
import type {
  PendingDictation,
  PendingDictationRequest,
} from "../../../shared/domain/processing";
import { processDemoText } from "../../dictation";

export function useOverlayDemo(getPreferences?: () => Preferences) {
  const phase = ref<Phase>("listening");
  const seconds = ref(3);
  const level = ref(0.6);
  const live = ref<LiveDictation | null>(null);
  const pending = ref<PendingDictation | null>(null);
  const resultText = ref("");
  let ticks = 0,
    revision = 0,
    nextSession = 0;
  let isSequence = false;
  let transition: ReturnType<typeof setTimeout> | undefined;
  let settle: (() => void) | undefined;
  const sequence: Phase[] = [
    "listening",
    "silence",
    "transcribing",
    "awaiting_action",
  ];

  function resetTransition() {
    revision++;
    isSequence = false;
    if (transition) clearTimeout(transition);
    transition = undefined;
    settle?.();
    settle = undefined;
  }
  function delay(milliseconds: number) {
    return new Promise<void>((resolve) => {
      settle = resolve;
      transition = setTimeout(() => {
        transition = undefined;
        settle = undefined;
        resolve();
      }, milliseconds);
    });
  }
  function waiting(error: string | null = null) {
    const preferences = getPreferences?.();
    pending.value = {
      sessionId: ++nextSession,
      phase: "awaiting_action",
      originalText:
        "Так, давайте, ну, оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.",
      resultText: null,
      createdAt: new Date().toISOString(),
      preset: preferences?.processingMode || "clean",
      targetLanguage:
        preferences?.processingTranslation &&
        preferences.processingTranslation !== "none"
          ? preferences.processingTranslation
          : null,
      processingEnabled: preferences?.processingEnabled ?? true,
      source: "hotkey",
      error,
      insertionBlocked: false,
    };
    phase.value = "awaiting_action";
    level.value = 0;
    isSequence = false;
  }
  const timer = setInterval(() => {
    ticks++;
    level.value = ["listening", "silence"].includes(phase.value)
      ? Math.abs(Math.sin(ticks * 0.4)) * 0.8
      : 0;
    if (phase.value === "silence")
      seconds.value = Math.max(0, 3 - (Math.floor(ticks / 5) % 4));
    if (isSequence && ticks % 18 === 0) {
      const next = sequence[sequence.indexOf(phase.value) + 1];
      if (next === "awaiting_action") waiting();
      else if (next) phase.value = next;
      else isSequence = false;
    }
  }, 200);
  onScopeDispose(() => {
    resetTransition();
    clearInterval(timer);
  });

  function select(value: Phase) {
    resetTransition();
    live.value = null;
    pending.value = null;
    resultText.value = "";
    phase.value = value;
    ticks = 0;
    seconds.value = 3;
    if (value === "awaiting_action") waiting();
  }
  function cancel() {
    resetTransition();
    pending.value = null;
    resultText.value = "";
    phase.value = "cancelled";
    if (live.value) live.value.phase = "cancelled";
  }
  async function finish() {
    if (!["listening", "silence"].includes(phase.value)) return;
    resetTransition();
    const ticket = revision;
    const capturedLive = live.value;
    phase.value = "transcribing";
    if (capturedLive) capturedLive.phase = "draining";
    await delay(700);
    if (ticket !== revision) return;
    if (capturedLive) {
      capturedLive.phase = "done";
      phase.value = "done";
    } else waiting();
  }
  async function resolve(request: PendingDictationRequest) {
    const current = pending.value;
    if (!current || current.sessionId !== request.sessionId) return;
    if (request.action === "cancel") return cancel();
    if (current.phase === "processing") return;
    if (request.action === "insert_raw") {
      resetTransition();
      resultText.value = current.originalText;
      pending.value = null;
      phase.value = "done";
      return;
    }
    if (!current.processingEnabled) return;
    resetTransition();
    const ticket = revision;
    current.phase = "processing";
    current.error = null;
    current.preset = request.preset || current.preset;
    if (request.targetLanguage !== undefined)
      current.targetLanguage = request.targetLanguage;
    phase.value = "processing";
    await delay(850);
    if (ticket !== revision || pending.value?.sessionId !== current.sessionId)
      return;
    resultText.value = processDemoText(
      current.originalText,
      current.preset,
      current.targetLanguage,
    );
    pending.value = null;
    phase.value = "done";
  }
  return {
    phase,
    seconds,
    level,
    live,
    pending,
    resultText,
    select,
    finish,
    cancel,
    resolve,
    showPendingError() {
      select("awaiting_action");
      pending.value!.error =
        "Пример ошибки подключения. Повторите обработку или вставьте исходный текст.";
    },
    selectLive(state: "recording" | "paused" | "backlog" | "error") {
      select("listening");
      seconds.value = 0;
      live.value = {
        sessionId: "overlay-demo",
        revision: 1,
        committedText: "Подтверждённая мысль уже в поле.",
        draftText: "Следующая фраза уточняется",
        pendingText:
          state === "paused" ? "Подтверждённая мысль уже в поле." : "",
        insertionState:
          state === "paused"
            ? "paused_focus"
            : state === "error"
              ? "failed"
              : "active",
        phase: "listening",
        lagMs: state === "backlog" ? 8500 : 2200,
      };
    },
    resumeLive() {
      if (live.value && live.value.insertionState !== "failed")
        live.value.insertionState = "active";
    },
    play() {
      select("listening");
      isSequence = true;
    },
  };
}
