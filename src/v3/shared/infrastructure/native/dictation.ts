import type { NativeContext } from "./context";
import type { DictationPort, Phase } from "../../domain/contracts";
import type { Transcript } from "../../../../lib/types";
import { call } from "./ipc";
import { createNativeSessionSync } from "./sessionSync";
import { createNativeResultObserver } from "./sessionResult";
import {
  assertDraftAvailable,
  liveIsActive,
  livePhase,
} from "../../../features/dictation";
import type { PendingDictation } from "../../domain/processing";
import type { NativeSnapshot, NativeLiveSnapshot } from "./dictationContracts";
export type {
  NativeResult,
  NativeSnapshot,
  NativeLiveSnapshot,
} from "./dictationContracts";
export function nativeDictation(ctx: NativeContext) {
  const { state } = ctx;
  let revision = 0;
  let started = 0;
  let currentOperation = -1;
  let completed = false;
  const retiredSessions = new Set<string>();
  const accept = createNativeResultObserver(ctx, retiredSessions, () => {
    revision++;
    completed = true;
  });
  const resolving = new Map<number, number>();
  const sync = createNativeSessionSync({
    operation: () => currentOperation,
    pending,
    live,
    snapshot,
  });
  function pending(value: PendingDictation | null) {
    if (value && value.sessionId < currentOperation) return;
    state.pendingDictation = value;
    if (value) {
      currentOperation = value.sessionId;
      state.phase = value.phase;
      state.error = value.error || "";
    }
  }
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
        if (
          state.pendingDictation &&
          state.pendingDictation.sessionId < value.operation_id
        )
          state.pendingDictation = null;
        state.error = "";
        revision++;
      }
    }
    if (value.last) accept(value.last);
    if (
      state.pendingDictation &&
      value.operation_id <= state.pendingDictation.sessionId
    ) {
      state.phase = state.pendingDictation.phase;
      return;
    }
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
    async resolvePending(request) {
      if (state.pendingDictation?.sessionId !== request.sessionId) return;
      if (resolving.has(request.sessionId) && request.action !== "cancel")
        return;
      resolving.set(
        request.sessionId,
        (resolving.get(request.sessionId) || 0) + 1,
      );
      sync.invalidate();
      try {
        // This IPC returns Transcript, without LastDictation's id/date/original.
        // The session event/snapshot is the authority for the displayed result.
        await call<Transcript | null>("resolve_pending_dictation", { request });
        if (currentOperation !== request.sessionId) return;
        await sync.refresh();
      } catch (error) {
        if (
          currentOperation === request.sessionId &&
          state.pendingDictation?.sessionId === request.sessionId
        )
          throw error;
      } finally {
        const remaining = (resolving.get(request.sessionId) || 1) - 1;
        if (remaining) resolving.set(request.sessionId, remaining);
        else resolving.delete(request.sessionId);
      }
    },
    async start() {
      if (
        [
          "listening",
          "silence",
          "transcribing",
          "processing",
          "awaiting_action",
        ].includes(state.phase)
      )
        return;
      revision++;
      sync.invalidate();
      completed = false;
      state.error = "";
      if (state.live) retiredSessions.add(state.live.sessionId);
      state.live = null;
      await call("start_dictation");
    },
    async finish() {
      await call(
        state.recordingSource === "wake_word"
          ? "confirm_dictation"
          : "stop_dictation",
      );
    },
    async cancel() {
      revision++;
      const operation = currentOperation;
      sync.invalidate();
      await call("cancel_dictation");
      if (currentOperation === operation) state.phase = "cancelled";
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
  function observe<T>(handler: (value: T) => void) {
    return (value: T) => {
      sync.invalidate();
      handler(value);
    };
  }
  return {
    port,
    refresh: sync.refresh,
    accept: observe(accept),
    snapshot: observe(snapshot),
    live: observe(live),
    pending(value: PendingDictation | null) {
      sync.invalidate();
      if (value) pending(value);
      // A nullable event carries no session id; confirm against native state.
      else void sync.refresh().catch(ctx.report);
    },
  };
}
