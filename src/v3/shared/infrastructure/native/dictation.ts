import type { NativeContext } from "./context";
import type { DictationPort, Dictation, Phase } from "../../domain/contracts";
import type { PipelineState } from "../../../../lib/types";
import { call } from "./ipc";
import {
  assertDraftAvailable,
  liveIsActive,
  livePhase,
} from "../../../features/dictation";
import type { LiveDictation } from "../../domain/contracts";
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
export interface NativeLiveSnapshot {
  session_id: string;
  revision: number;
  committed_text: string;
  draft_text: string;
  pending_text: string;
  insertion_state: LiveDictation["insertionState"];
  lag_ms: number;
  phase: LiveDictation["phase"];
  source: "ui" | "hotkey" | "wake_word";
  elapsed_ms: number;
  audio_level: number;
  warning?: string | null;
}
export function nativeDictation(ctx: NativeContext) {
  const { state } = ctx;
  let revision = 0;
  let started = 0;
  let currentOperation = -1;
  let completed = false;
  const retiredSessions = new Set<string>();
  const acceptedResults = new Map<string, Set<string>>();
  function live(value: NativeLiveSnapshot | null) {
    if (
      !value ||
      state.preferences.dictationMode !== "live" ||
      retiredSessions.has(value.session_id)
    )
      return;
    if (Number(value.session_id) < currentOperation) return;
    const previous = state.live;
    if (
      previous?.sessionId === value.session_id &&
      previous.revision >= value.revision
    ) {
      if (previous.revision === value.revision) {
        state.elapsed = value.elapsed_ms / 1000;
        state.audioLevel = Math.min(1, Math.max(0, value.audio_level));
      }
      return;
    }
    if (previous && previous.sessionId !== value.session_id) {
      retiredSessions.add(previous.sessionId);
      if (retiredSessions.size > 128)
        retiredSessions.delete(retiredSessions.values().next().value!);
    }
    revision++;
    state.live = {
      sessionId: value.session_id,
      revision: value.revision,
      committedText: value.committed_text,
      draftText: value.draft_text,
      pendingText: value.pending_text,
      insertionState: value.insertion_state,
      phase: value.phase,
      lagMs: value.lag_ms,
      warning: value.warning || undefined,
    };
    state.phase = livePhase(state.live);
    state.recordingSource = value.source;
    state.elapsed = value.elapsed_ms / 1000;
    state.audioLevel = Math.min(1, Math.max(0, value.audio_level));
    if (value.phase === "error")
      state.error =
        value.warning ||
        "Живая диктовка прервана. Подтверждённый текст сохранён.";
  }
  function accept(result: NativeResult) {
    if (retiredSessions.has(result.id)) return;
    const payload = JSON.stringify(result);
    if (acceptedResults.get(result.id)?.has(payload)) return;
    if (acceptedResults.has(result.id) && state.last.entry?.id !== result.id)
      return;
    const versions = acceptedResults.get(result.id) || new Set<string>();
    versions.add(payload);
    acceptedResults.set(result.id, versions);
    if (acceptedResults.size > 128)
      acceptedResults.delete(acceptedResults.keys().next().value!);
    revision++;
    const entry: Dictation = {
      id: result.id,
      text: result.text,
      original: result.original_text,
      createdAt: result.created_at,
      duration: result.audio_secs,
      title: result.text.slice(0, 64),
    };
    state.last =
      state.last.entry?.id === result.id && state.last.edited
        ? { ...state.last, entry }
        : {
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
    if (value.operation_id > 0 && value.operation_id < currentOperation) return;
    if (
      state.live &&
      value.operation_id > Number(state.live.sessionId) &&
      value.state !== "idle"
    ) {
      retiredSessions.add(state.live.sessionId);
      state.live = null;
    }
    if (value.operation_id > 0 && value.operation_id !== currentOperation) {
      currentOperation = value.operation_id;
      started = Date.now();
      completed = false;
      if (value.state === "listening") {
        state.error = "";
        revision++;
      }
    }
    if (value.last) accept(value.last);
    if (state.live && state.preferences.dictationMode === "live") return;
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
      if (
        ["listening", "silence", "transcribing", "processing"].includes(
          state.phase,
        )
      )
        return;
      revision++;
      completed = false;
      state.error = "";
      if (state.live) retiredSessions.add(state.live.sessionId);
      state.live = null;
      await call("start_dictation");
    },
    async finish() {
      if (
        state.recordingSource === "hotkey" &&
        state.preferences.dictationMode !== "live"
      )
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
    async resumeInsertion() {
      if (!liveIsActive(state.live) || state.live?.phase !== "listening")
        throw new Error("Живая диктовка уже завершена.");
      if (state.live?.insertionState === "failed")
        throw new Error(
          "Вставка остановлена. Проверьте поле и скопируйте остаток вручную.",
        );
      await call("resume_live_insertion");
    },
    chooseVariant(variant) {
      assertDraftAvailable(state.live);
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
      assertDraftAvailable(state.live);
      revision++;
      state.last.draft = text;
      state.last.edited = true;
    },
    async improve() {
      assertDraftAvailable(state.live);
      if (state.preferences.dictationMode === "live")
        throw new Error("В живом режиме обработка через ИИ отключена.");
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
  return { port, accept, snapshot, live };
}
