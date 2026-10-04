import type { Phase } from "../../../shared/domain/contracts";
import type {
  PendingDictation,
  PendingDictationRequest,
} from "../../../shared/domain/processing";
import { call } from "../../../shared/infrastructure/native/ipc";

interface PendingState {
  pending: PendingDictation | null;
  phase: Phase;
  source: string | null;
  level: number;
  error: string;
}

/** Events win over older poll replies; retired sessions cannot reappear. */
export function createPendingOverlay(
  state: PendingState,
  disposed: () => boolean,
) {
  let epoch = 0,
    latestSession = 0,
    retiredSession = 0,
    action = 0,
    busySession: number | null = null;
  const stamp = () => epoch;
  const current = (ticket: number) => !disposed() && ticket === epoch;
  function apply(value: PendingDictation | null, ticket?: number) {
    if (disposed() || (ticket !== undefined && !current(ticket))) return;
    const previousSession = state.pending?.sessionId;
    if (value) {
      if (
        !Number.isSafeInteger(value.sessionId) ||
        value.sessionId <= retiredSession ||
        value.sessionId < latestSession
      )
        return;
      if (
        ticket !== undefined &&
        busySession === value.sessionId &&
        state.pending?.phase === "processing" &&
        value.phase === "awaiting_action" &&
        !value.error
      )
        return;
      latestSession = value.sessionId;
      state.pending = value;
      state.phase = value.phase;
      state.source = value.source;
      state.level = 0;
    } else {
      if (state.pending)
        retiredSession = Math.max(retiredSession, state.pending.sessionId);
      state.pending = null;
    }
    epoch++;
    if (previousSession !== value?.sessionId || value?.error) state.error = "";
  }
  function observeOperation(id: number) {
    if (id <= latestSession) return;
    if (state.pending) apply(null);
    latestSession = id;
  }
  async function resolve(request: PendingDictationRequest) {
    if (
      disposed() ||
      state.pending?.sessionId !== request.sessionId ||
      (busySession === request.sessionId && request.action !== "cancel")
    )
      return;
    const ownAction = ++action;
    busySession = request.sessionId;
    epoch++;
    state.error = "";
    if (request.action !== "cancel") {
      state.pending = { ...state.pending, phase: "processing", error: null };
      state.phase = "processing";
    }
    try {
      await call("resolve_pending_dictation", { request });
      if (
        disposed() ||
        ownAction !== action ||
        state.pending?.sessionId !== request.sessionId
      )
        return;
      apply(null);
      state.phase = request.action === "cancel" ? "cancelled" : "done";
    } catch (error) {
      if (
        disposed() ||
        ownAction !== action ||
        state.pending?.sessionId !== request.sessionId
      )
        return;
      state.pending = { ...state.pending, phase: "awaiting_action" };
      state.phase = "awaiting_action";
      state.error = error instanceof Error ? error.message : String(error);
      epoch++;
    } finally {
      if (ownAction === action) busySession = null;
    }
  }
  async function copy(text: string) {
    const session = state.pending?.sessionId;
    try {
      await call("copy_dictation_text", { text });
    } catch (error) {
      if (!disposed() && state.pending?.sessionId === session)
        state.error = error instanceof Error ? error.message : String(error);
    }
  }
  return { stamp, current, apply, observeOperation, resolve, copy };
}
