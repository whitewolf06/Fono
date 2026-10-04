import { getCurrentScope, onScopeDispose, reactive } from "vue";
import type { IndicatorChoice, IndicatorPhase } from "../domain/indicator";
import { processDemoText } from "../../dictation";

export type OverlaySketchPhase = IndicatorPhase;
export type OverlayFinishOrigin = "button" | "hotkey";
export type OverlaySketchChoice = IndicatorChoice;
export interface OverlaySketchState extends OverlaySketchChoice {
  phase: OverlaySketchPhase;
  sessionId: number;
  elapsedMs: number;
  level: number;
  originalText: string;
  result: string;
  insertedText: string;
  error: string;
  copying: boolean;
  finishOrigin: OverlayFinishOrigin | null;
  frozenChoice: Readonly<OverlaySketchChoice> | null;
}
export interface OverlaySketchOptions {
  copy: (text: string) => Promise<void>;
  wait: (milliseconds: number) => Promise<void>;
  subscribeTick: (callback: () => void, intervalMs: number) => () => void;
  onInsert?: (text: string) => void;
  initial?: Partial<OverlaySketchChoice>;
  autoStart?: boolean;
  transcriptionMs?: number;
  processingMs?: number;
}

const sample =
  "Так, давайте, ну, оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.";
const tickMs = 120;

/** In-memory browser concept. It never records audio or inserts into real fields. */
export function useOverlaySketch(options: OverlaySketchOptions) {
  const state = reactive<OverlaySketchState>({
    phase: "closed",
    sessionId: 0,
    elapsedMs: 0,
    level: 0,
    originalText: "",
    result: "",
    insertedText: "",
    error: "",
    copying: false,
    finishOrigin: null,
    frozenChoice: null,
    postprocessingOn: true,
    translationOn: false,
    style: "clean",
    language: "en",
    ...options.initial,
  });
  let revision = 0;
  let disposed = false;
  const unsubscribe = options.subscribeTick(() => {
    if (disposed || state.phase !== "recording") return;
    state.elapsedMs += tickMs;
    const beat = state.elapsedMs / 1000;
    state.level = Math.min(
      1,
      0.12 + Math.abs(Math.sin(beat * 2.4) * Math.cos(beat * 0.7)) * 0.88,
    );
  }, tickMs);

  function current(ticket: number, sessionId: number) {
    return !disposed && revision === ticket && state.sessionId === sessionId;
  }
  function choice(): OverlaySketchChoice {
    return {
      postprocessingOn: state.postprocessingOn,
      translationOn: state.translationOn,
      style: state.style,
      language: state.language,
    };
  }
  function start() {
    if (disposed) return;
    revision++;
    state.sessionId++;
    state.phase = "recording";
    state.elapsedMs = 0;
    state.level = 0.36;
    state.originalText = "";
    state.result = "";
    state.insertedText = "";
    state.error = "";
    state.copying = false;
    state.finishOrigin = null;
    state.frozenChoice = null;
  }
  function choose(patch: Partial<OverlaySketchChoice>) {
    if (disposed || !["recording", "closed", "error"].includes(state.phase))
      return;
    Object.assign(state, patch);
  }
  function close() {
    if (disposed) return;
    revision++;
    state.phase = "closed";
    state.level = 0;
    state.copying = false;
    state.result = "";
    state.originalText = "";
    state.error = "";
  }
  async function finish(origin: OverlayFinishOrigin = "button") {
    if (disposed || state.phase !== "recording") return;
    const ticket = ++revision;
    const sessionId = state.sessionId;
    const captured = Object.freeze(choice());
    state.frozenChoice = captured;
    state.finishOrigin = origin;
    state.level = 0;
    state.phase = "transcribing";
    try {
      await options.wait(options.transcriptionMs ?? 650);
      if (!current(ticket, sessionId)) return;
      state.originalText = sample;
      let result = sample;
      if (captured.postprocessingOn) {
        state.phase = "processing";
        await options.wait(options.processingMs ?? 800);
        if (!current(ticket, sessionId)) return;
        result = processDemoText(
          sample,
          captured.style,
          captured.translationOn ? captured.language : null,
        );
      }
      state.result = result;
      if (origin === "hotkey") {
        options.onInsert?.(result);
        state.insertedText = result;
        // Callback may synchronously restart the preview; never close that session.
        if (!current(ticket, sessionId)) return;
        close();
      } else state.phase = "ready";
    } catch {
      if (!current(ticket, sessionId)) return;
      const failedInsertion = Boolean(state.result);
      state.result ||= state.originalText;
      state.phase = "error";
      state.error = failedInsertion
        ? "Не удалось вставить демотекст. Результат можно скопировать."
        : state.result
          ? "Демонстрационная обработка прервана. Исходный текст можно скопировать."
          : "Демонстрационное распознавание прервано. Повторите запись.";
    }
  }
  async function copy() {
    if (
      disposed ||
      state.copying ||
      !["ready", "error"].includes(state.phase) ||
      !state.result
    )
      return;
    const ticket = revision;
    const sessionId = state.sessionId;
    state.copying = true;
    state.error = "";
    try {
      await options.copy(state.result);
      if (current(ticket, sessionId)) close();
    } catch {
      if (!current(ticket, sessionId)) return;
      state.phase = "ready";
      state.error =
        "Не удалось скопировать. Повторите попытку — текст сохранён.";
    } finally {
      if (current(ticket, sessionId)) state.copying = false;
    }
  }
  function dispose() {
    if (disposed) return;
    disposed = true;
    revision++;
    unsubscribe();
    state.level = 0;
  }
  if (getCurrentScope()) onScopeDispose(dispose);
  if (options.autoStart !== false) start();
  return { state, start, choose, finish, copy, close, cancel: close, dispose };
}
