import { message as uiMessage } from "../../../shared/domain/messages";
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
  clock = { setInterval, clearInterval },
): DictationPort & { dispose(): void } {
  let generation = 0;
  let snapshot = capturePreferences(state);
  let timer: ReturnType<typeof setInterval> | undefined;
  const pending = createPendingWorkflow(state, delay, () => port.cancel());
  const liveWords =
    typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
    __FONO_EXPERIMENTAL_LIVE__
      ? "Сегодня мы проверим живую диктовку. Да, да, естественные повторы останутся. Подтверждённый текст появляется последовательно и больше не переписывается.".split(
          " ",
        )
      : [];
  function stopTimer() {
    if (timer) clock.clearInterval(timer);
    timer = undefined;
    state.audioLevel = 0;
    state.countdown = undefined;
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
        state.error = uiMessage("ui.8fd5783bb6");
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
        state.error = uiMessage("ui.566676488a");
        state.phase = "error";
        return;
      }
      reset();
      state.error = "";
      state.dictationOutcome = undefined;
      state.phase = "listening";
      state.elapsed = 0;
      state.recordingSource = "ui";
      snapshot = capturePreferences(state);
      state.live =
        typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
        __FONO_EXPERIMENTAL_LIVE__ &&
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
      const activeGeneration = generation;
      let elapsedTicks = 0;
      timer = clock.setInterval(() => {
        if (generation !== activeGeneration) return;
        elapsedTicks++;
        state.elapsed = elapsedTicks / 10;
        state.audioLevel = Math.max(
          0.08,
          (Math.sin(state.elapsed * 4) + Math.cos(state.elapsed * 2.4) + 2) / 4,
        );
        if (
          typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
          __FONO_EXPERIMENTAL_LIVE__ &&
          state.live
        ) {
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
        } else if (
          (state.scenario === "no-speech" || state.elapsed >= 10) &&
          (snapshot.preferences.wakeEnabled ||
            (snapshot.preferences.hotkeyMode === "toggle" &&
              snapshot.preferences.toggleAutoStopMs >= 2000))
        ) {
          const quietStarted = state.scenario === "no-speech" ? 0 : 10;
          const timeout = snapshot.preferences.wakeEnabled
            ? snapshot.preferences.silenceMs
            : snapshot.preferences.toggleAutoStopMs;
          state.phase = "silence";
          state.audioLevel = 0;
          state.countdown = {
            remaining_ms: Math.max(
              0,
              timeout - (state.elapsed - quietStarted) * 1000,
            ),
            timeout_ms: timeout,
            speaking: false,
          };
          if (state.elapsed >= quietStarted + timeout / 1000)
            void port.finish();
        }
      }, 100);
    },
    async finish() {
      if (!["listening", "silence"].includes(state.phase)) return;
      stopTimer();
      if (state.scenario === "no-speech") {
        generation++;
        state.dictationOutcome = "no_speech";
        state.phase = "idle";
        return;
      }
      const ticket = ++generation;
      const generationStarted = Date.now();
      const captured = snapshot;
      const preferences = captured.preferences;
      const targetLanguage =
        preferences.processingTranslationEnabled !== false &&
        preferences.processingTranslation !== "none"
          ? preferences.processingTranslation
          : null;
      const live =
        typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
        __FONO_EXPERIMENTAL_LIVE__
          ? state.live
          : null;
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
      if (
        !live &&
        preferences.processingEnabled &&
        (preferences.processingMode !== "raw" || targetLanguage)
      ) {
        state.phase = "processing";
        const started = Date.now();
        await delay(800);
        if (ticket !== generation) return;
        processingDurationMs = Date.now() - started;
        if (!state.aiAvailable) {
          pending.enqueue(
            { sessionId: ticket, entry, snapshot: captured, generationStarted },
            uiMessage("ui.77a153c450"),
          );
          return;
        }
        text = processDemoText(
          original,
          preferences.processingMode,
          targetLanguage,
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
      if (
        typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
        __FONO_EXPERIMENTAL_LIVE__ &&
        state.live
      ) {
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
      if (!(
        typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
        __FONO_EXPERIMENTAL_LIVE__
      ))
        return;
      if (!state.live || state.live.phase !== "listening")
        throw new Error(uiMessage("ui.347c573ebe"));
      if (state.live.insertionState === "failed")
        throw new Error(uiMessage("ui.0cf5b061d5"));
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
