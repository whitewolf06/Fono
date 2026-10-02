import type { NativeContext } from "./context";
import type { DictationPort, Dictation, Phase } from "../../domain/contracts";
import type { PipelineState } from "../../../../lib/types";
import { call } from "./ipc";
export interface NativeResult {
  id: string;
  text: string;
  original_text: string;
  created_at: string;
  audio_secs: number;
}
export interface NativeSnapshot {
  state: PipelineState;
  level: number;
  last: NativeResult | null;
  operation_id: number;
  source: string | null;
}
export function nativeDictation(ctx: NativeContext) {
  const { state } = ctx;
  let revision = 0;
  let started = 0;
  let currentOperation = -1;
  let completed = false;
  function accept(result: NativeResult) {
    if (state.last.entry?.id === result.id) return;
    revision++;
    const entry: Dictation = {
      id: result.id,
      text: result.text,
      original: result.original_text,
      createdAt: result.created_at,
      duration: result.audio_secs,
      title: result.text.slice(0, 64),
    };
    state.last = {
      entry,
      variant: "result",
      draft: result.text,
      edited: false,
      undo: null,
    };
    completed = true;
    void ctx.readHistory().catch(ctx.report);
  }
  function snapshot(value: NativeSnapshot) {
    if (value.operation_id !== currentOperation) {
      currentOperation = value.operation_id;
      started = Date.now();
      completed = false;
      if (value.state === "listening") {
        state.error = "";
        revision++;
      }
    }
    if (value.last) accept(value.last);
    state.recordingSource = value.source || undefined;
    state.audioLevel = Math.min(1, value.level * 8);
    state.testSignal = state.audioLevel;
    if (value.state === "listening")
      state.elapsed = (Date.now() - started) / 1000;
    state.phase =
      value.state === "injecting"
        ? "processing"
        : value.state === "idle"
          ? completed
            ? "done"
            : "idle"
          : value.state === "listening" &&
              state.countdown &&
              !state.countdown.speaking &&
              state.recordingSource === "wake_word"
            ? "silence"
            : (value.state as Phase);
  }
  const port: DictationPort = {
    async start() {
      revision++;
      completed = false;
      state.error = "";
      await call("start_dictation");
    },
    async finish() {
      if (state.recordingSource === "hotkey")
        throw new Error("Отпустите горячую клавишу, чтобы завершить запись");
      await call(
        state.recordingSource === "wake_word"
          ? "confirm_dictation"
          : "stop_dictation",
      );
    },
    async cancel() {
      revision++;
      await call("cancel_dictation");
      state.phase = "cancelled";
    },
    chooseVariant(variant) {
      if (!state.last.entry) return;
      revision++;
      state.last.variant = variant;
      state.last.draft =
        variant === "original"
          ? state.last.entry.original || state.last.entry.text
          : state.last.entry.text;
      state.last.edited = false;
      state.last.undo = null;
    },
    edit(text) {
      revision++;
      state.last.draft = text;
      state.last.edited = true;
    },
    async improve() {
      const currentRevision = ++revision;
      const previous = state.last.draft;
      const text = await call<string>("improve_text", { text: previous });
      if (revision !== currentRevision)
        throw new Error(
          "Черновик изменился во время обработки. Результат не применён.",
        );
      state.last.undo = previous;
      state.last.draft = text;
      state.last.edited = true;
    },
    undoImprove() {
      if (state.last.undo !== null) {
        revision++;
        state.last.draft = state.last.undo;
        state.last.undo = null;
      }
    },
  };
  return { port, accept, snapshot };
}
