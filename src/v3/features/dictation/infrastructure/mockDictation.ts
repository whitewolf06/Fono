import type {
  DictationPort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import { canonicalizeDictionary } from "../../../shared/domain/personalDictionary";
import {
  capturePreferences,
  createDemoEntry,
  publishDemoEntry,
  selectLatest,
} from "./mockSession";
import { createPendingWorkflow } from "./mockPending";
import { createMockDraftActions } from "./mockDraft";
import { processDemoText } from "./mockText";
export { selectLatest } from "./mockSession";

const wait = (milliseconds: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

export function createDictationPort(
  state: WorkspaceState,
  delay: (milliseconds: number) => Promise<void> = wait,
): DictationPort & { dispose(): void } {
  let generation = 0;
  let snapshot = capturePreferences(state);
  let timer: ReturnType<typeof setInterval> | undefined;
  const pending = createPendingWorkflow(state, delay, () => port.cancel());
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
    pending.clear();
  }
  const port: DictationPort & { dispose(): void } = {
    start() {
      if (
        state.pendingDictation ||
        [
          "listening",
          "silence",
          "transcribing",
          "processing",
          "awaiting_action",
        ].includes(state.phase)
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
          (model) =>
            model.id === state.preferences.model &&
            model.status === "installed",
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
      snapshot = capturePreferences(state);
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
        } else if (state.elapsed >= 10 && snapshot.preferences.wakeEnabled) {
          state.phase = "silence";
          state.audioLevel = 0.03;
          if (state.elapsed >= 10 + snapshot.preferences.silenceMs / 1000)
            void port.finish();
        }
      }, 100);
    },
    async finish() {
      if (!["listening", "silence"].includes(state.phase)) return;
      stopTimer();
      const ticket = ++generation;
      const generationStarted = Date.now();
      const captured = snapshot;
      const preferences = captured.preferences;
      const live = state.live;
      state.phase = "transcribing";
      if (live) {
        live.phase = "draining";
        live.revision++;
      }
      await delay(800);
      if (ticket !== generation) return;
      const original = live
        ? [live.committedText, live.draftText].filter(Boolean).join(" ") ||
          liveWords.slice(0, 6).join(" ")
        : "Так, давайте, ну, оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.";
      const entry = createDemoEntry(
        state,
        captured,
        original,
        ticket,
        generationStarted,
        Date.now() - generationStarted,
      );
      if (
        !live &&
        preferences.processingEnabled &&
        preferences.processingTrigger === "manual"
      ) {
        pending.enqueue({
          sessionId: ticket,
          entry,
          snapshot: captured,
          generationStarted,
        });
        return;
      }
      let text = original;
      let processingDurationMs: number | null = null;
      if (!live && preferences.processingEnabled) {
        state.phase = "processing";
        const started = Date.now();
        await delay(800);
        if (ticket !== generation) return;
        processingDurationMs = Date.now() - started;
        if (!state.aiAvailable) {
          pending.enqueue(
            { sessionId: ticket, entry, snapshot: captured, generationStarted },
            "Демонстрационная модель недоступна. Повторите обработку или вставьте исходный текст.",
          );
          return;
        }
        text = processDemoText(
          original,
          preferences.processingMode,
          preferences.processingTranslation === "none"
            ? null
            : preferences.processingTranslation,
        );
      }
      if (!live) text = canonicalizeDictionary(preferences, text);
      publishDemoEntry(
        state,
        captured,
        entry,
        text,
        !live && preferences.processingEnabled
          ? preferences.processingMode
          : "off",
        generationStarted,
        processingDurationMs,
      );
      state.phase = "done";
      if (live) {
        live.phase = "done";
        live.committedText = text;
        live.draftText = "";
        live.revision++;
      }
    },
    resolvePending: pending.resolve,
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
    ...createMockDraftActions(state, delay),
    dispose: reset,
  };
  return port;
}
