import type {
  DictationPort,
  WorkspaceState,
  Dictation,
} from "../../../shared/domain/contracts";
import { assertDraftAvailable } from "../domain/live";
import { canonicalizeDictionary } from "../../../shared/domain/personalDictionary";
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
  let recognitionSnapshot = recognitionPreferences();
  function recognitionPreferences() {
    const p = state.preferences;
    return {
      model: state.models.find((m) => m.id === p.model)?.name || p.model,
      acceleration: p.acceleration,
      language: p.language,
      processingEnabled: p.processingEnabled,
      processingMode: p.processingMode,
    };
  }
  let dictionarySnapshot = {
    dictionaryEnabled: state.preferences.dictionaryEnabled,
    dictionaryEntries: state.preferences.dictionaryEntries,
  };
  let timer: ReturnType<typeof setInterval> | undefined;
  const liveWords =
    "Сегодня мы проверим живую диктовку. Да, да, естественные повторы останутся. Подтверждённый текст появляется последовательно и больше не переписывается.".split(
      " ",
    );
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
      state.recordingSource = "ui";
      recognitionSnapshot = recognitionPreferences();
      dictionarySnapshot = {
        dictionaryEnabled: state.preferences.dictionaryEnabled,
        dictionaryEntries: state.preferences.dictionaryEntries.map((entry) => ({
          ...entry,
          spoken: [...entry.spoken],
        })),
      };
      state.live =
        state.preferences.dictationMode === "live"
          ? {
              sessionId: "live-demo-" + Date.now(),
              revision: 1,
              committedText: "",
              draftText: "",
              pendingText: "",
              insertionState: "none",
              phase: "listening",
              lagMs: 2200,
            }
          : null;
      timer = setInterval(() => {
        state.elapsed += 0.1;
        state.audioLevel = Math.max(
          0.08,
          (Math.sin(state.elapsed * 4) + Math.cos(state.elapsed * 2.4) + 2) / 4,
        );
        if (state.live) {
          const count = Math.min(
            liveWords.length,
            Math.floor(state.elapsed * 1.5),
          );
          const stable = Math.max(0, count - 3);
          state.live.revision++;
          state.live.committedText = liveWords.slice(0, stable).join(" ");
          state.live.draftText = liveWords.slice(stable, count).join(" ");
          state.live.pendingText =
            state.live.insertionState === "active"
              ? ""
              : state.live.committedText;
        } else if (state.elapsed >= 10 && state.preferences.wakeEnabled) {
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
      const generationStarted = Date.now();
      state.phase = "transcribing";
      if (state.live) {
        state.live.phase = "draining";
        state.live.revision++;
      }
      await wait(800);
      if (ticket !== generation) return;
      const recognitionDurationMs = Date.now() - generationStarted;
      let processingDurationMs: number | null = null;
      const original = state.live
        ? [state.live.committedText, state.live.draftText]
            .filter(Boolean)
            .join(" ") || liveWords.slice(0, 6).join(" ")
        : "Так, давайте, ну, оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.";
      let text = original;
      if (
        recognitionSnapshot.processingEnabled &&
        state.preferences.dictationMode !== "live"
      ) {
        state.phase = "processing";
        const processingStarted = Date.now();
        await wait(800);
        if (ticket !== generation) return;
        processingDurationMs = Date.now() - processingStarted;
        if (state.aiAvailable)
          text =
            "Давайте оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.";
        else
          state.error =
            "Обработка недоступна. Исходная расшифровка сохранена в последнем тексте.";
      }
      if (state.preferences.dictationMode !== "live")
        text = canonicalizeDictionary(dictionarySnapshot, text);
      const entry: Dictation = {
        id: "session-" + Date.now(),
        createdAt: new Date().toISOString(),
        title: "Новая диктовка",
        original,
        text,
        duration: Math.max(1, Math.round(state.elapsed)),
        metadata: {
          demo: true,
          recordingDurationMs: Math.round(state.elapsed * 1000),
          generationDurationMs: Date.now() - generationStarted,
          recognitionDurationMs,
          processingDurationMs,
          model: recognitionSnapshot.model,
          language: recognitionSnapshot.language,
          requestedAcceleration: ["auto", "cpu", "cuda", "vulkan"].includes(
            recognitionSnapshot.acceleration,
          )
            ? (recognitionSnapshot.acceleration as
                "auto" | "cpu" | "cuda" | "vulkan")
            : undefined,
          backend: null,
          dictationMode: "standard",
          processingMode: recognitionSnapshot.processingEnabled
            ? recognitionSnapshot.processingMode
            : "off",
          dictionaryEnabled: dictionarySnapshot.dictionaryEnabled,
        },
      };
      selectLatest(state, entry);
      if (dictionarySnapshot.dictionaryEnabled && text !== original) {
        state.last.variant = "result";
        state.last.draft = text;
      }
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
      if (state.live) {
        state.live.phase = "done";
        state.live.committedText = text;
        state.live.draftText = "";
        state.live.revision++;
      }
    },
    cancel() {
      reset();
      if (state.live) {
        state.live.phase = "cancelled";
        const available = [state.live.committedText, state.live.draftText]
          .filter(Boolean)
          .join(" ");
        state.live.draftText = "";
        state.live.revision++;
        if (available)
          selectLatest(state, {
            id: state.live.sessionId,
            createdAt: new Date().toISOString(),
            text: available,
            original: available,
            duration: state.elapsed,
            title: "Отменённая живая диктовка",
          });
      }
      state.phase = "cancelled";
      state.error = "";
    },
    async resumeInsertion() {
      if (!state.live || state.live.phase !== "listening")
        throw new Error("Живая диктовка уже завершена.");
      if (state.live.insertionState === "failed")
        throw new Error(
          "Вставка остановлена. Проверьте поле и скопируйте остаток вручную.",
        );
      state.live.insertionState = "active";
      state.live.pendingText = "";
      state.live.warning = undefined;
      state.live.revision++;
    },
    chooseVariant(variant) {
      assertDraftAvailable(state.live);
      const entry = state.last.entry;
      if (!entry) return;
      state.last.variant = variant;
      state.last.draft =
        variant === "original" ? (entry.original ?? entry.text) : entry.text;
      state.last.edited = false;
      state.last.undo = null;
    },
    edit(text) {
      assertDraftAvailable(state.live);
      state.last.draft = text;
      state.last.edited = true;
      state.last.undo = null;
    },
    async improve() {
      assertDraftAvailable(state.live);
      if (state.preferences.dictationMode === "live")
        throw new Error("В живом режиме обработка через ИИ отключена.");
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
