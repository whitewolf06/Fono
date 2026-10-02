import type {
  DictationPort,
  WorkspaceState,
  Dictation,
} from "../../../shared/domain/contracts";
const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
export function selectLatest(state: WorkspaceState, entry: Dictation | null) {
  state.last = {
    entry,
    variant: entry?.original ? "original" : "result",
    draft: entry?.original ?? entry?.text ?? "",
    edited: false,
    undo: null,
  };
}
export function createDictationPort(
  state: WorkspaceState,
): DictationPort & { dispose(): void } {
  let generation = 0;
  let timer: ReturnType<typeof setInterval> | undefined;
  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = undefined;
    state.audioLevel = 0;
  }
  function reset() {
    generation++;
    stopTimer();
  }
  const port: DictationPort & { dispose(): void } = {
    start() {
      if (
        ["listening", "silence", "transcribing", "processing"].includes(
          state.phase,
        )
      )
        return;
      if (!state.microphoneAvailable) {
        state.error =
          "Микрофон недоступен. Выберите устройство в настройках аудио.";
        state.phase = "error";
        return;
      }
      if (
        !state.models.some(
          (m) => m.id === state.preferences.model && m.status === "installed",
        )
      ) {
        state.error =
          "Модель не установлена. Загрузите её в настройках распознавания.";
        state.phase = "error";
        return;
      }
      reset();
      state.error = "";
      state.phase = "listening";
      state.elapsed = 0;
      timer = setInterval(() => {
        state.elapsed += 0.1;
        state.audioLevel = Math.max(
          0.08,
          (Math.sin(state.elapsed * 4) + Math.cos(state.elapsed * 2.4) + 2) / 4,
        );
        if (state.elapsed >= 10 && state.preferences.wakeEnabled) {
          state.phase = "silence";
          state.audioLevel = 0.03;
          if (state.elapsed >= 10 + state.preferences.silenceMs / 1000)
            void port.finish();
        }
      }, 100);
    },
    async finish() {
      if (!["listening", "silence"].includes(state.phase)) return;
      stopTimer();
      const ticket = ++generation;
      state.phase = "transcribing";
      await wait(800);
      if (ticket !== generation) return;
      const original =
        "Так, давайте, ну, оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.";
      let text = original;
      if (state.preferences.processingEnabled) {
        state.phase = "processing";
        await wait(800);
        if (ticket !== generation) return;
        if (state.aiAvailable)
          text =
            "Давайте оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.";
        else
          state.error =
            "Обработка недоступна. Исходная расшифровка сохранена в последнем тексте.";
      }
      const entry: Dictation = {
        id: "session-" + Date.now(),
        createdAt: new Date().toISOString(),
        title: "Новая диктовка",
        original,
        text,
        duration: Math.max(1, Math.round(state.elapsed)),
      };
      selectLatest(state, entry);
      if (state.preferences.historyEnabled) {
        const archived = { ...entry };
        if (
          !state.preferences.analyticsConsent ||
          !state.preferences.trainerEnabled
        )
          delete archived.original;
        state.history.unshift(archived);
      }
      state.phase = state.error ? "error" : "done";
    },
    cancel() {
      reset();
      state.phase = "cancelled";
      state.error = "";
    },
    chooseVariant(variant) {
      const entry = state.last.entry;
      if (!entry) return;
      state.last.variant = variant;
      state.last.draft =
        variant === "original" ? (entry.original ?? entry.text) : entry.text;
      state.last.edited = false;
      state.last.undo = null;
    },
    edit(text) {
      state.last.draft = text;
      state.last.edited = true;
      state.last.undo = null;
    },
    async improve() {
      if (!state.preferences.processingEnabled || !state.last.draft.trim())
        throw new Error(
          "Включите обработку текста, чтобы использовать улучшение.",
        );
      const entryId = state.last.entry?.id;
      const previous = state.last.draft;
      await wait(800);
      if (!state.aiAvailable)
        throw new Error(
          "Модель недоступна. Текст не изменён — проверьте подключение.",
        );
      if (state.last.entry?.id !== entryId || state.last.draft !== previous)
        return;
      state.last.undo = previous;
      const cleaned = previous
        .replace(/(^|[.!?]\s+)(Так,\s*|Ну,\s*)/g, "$1")
        .replace(/, ну,/gi, ",")
        .replace(/потом, потом/gi, "потом")
        .trim()
        .replace(/^[а-яёa-z]/u, (letter) => letter.toLocaleUpperCase("ru"));
      state.last.draft =
        state.preferences.processingMode === "format"
          ? cleaned
              .split(/(?<=\.)\s+/)
              .map((s) => "• " + s)
              .join("\n")
          : cleaned;
      state.last.edited = true;
    },
    undoImprove() {
      if (state.last.undo !== null) {
        state.last.draft = state.last.undo;
        state.last.undo = null;
      }
    },
    dispose: reset,
  };
  return port;
}
