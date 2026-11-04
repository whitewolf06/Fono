import type { RecoveryDictation, RecoveryRequest } from "../../domain/recovery";
import type { NativeDictationOutcome } from "./dictationContracts";
import { call } from "./ipc";

/** Action replies never revive an operation superseded or cancelled while awaiting IPC. */
export function createRecoveryActions(options: {
  current(): RecoveryDictation | null | undefined;
  operation(): number;
  recognizing(value: RecoveryDictation): void;
  invalidate(): void;
  refresh(): Promise<void>;
  outcome(operation: number, result: NativeDictationOutcome): void;
  disposed?(): boolean;
}) {
  let epoch = 0;
  let busy = false;
  return async (request: RecoveryRequest): Promise<void> => {
    const current = options.current();
    if (
      !current ||
      current.session_id !== request.session_id ||
      options.disposed?.()
    )
      return;
    if (
      request.action !== "cancel" &&
      (busy || current.phase === "recognizing")
    )
      return;
    const ticket = ++epoch;
    busy = true;
    options.invalidate();
    if (request.action !== "cancel")
      options.recognizing({ ...current, phase: "recognizing" });
    const active = () =>
      !options.disposed?.() &&
      ticket === epoch &&
      options.operation() === request.session_id;
    try {
      const result = await call<NativeDictationOutcome>(
        "resolve_recovery_dictation",
        { request },
      );
      if (!active()) return;
      await options.refresh();
      if (active() && result) options.outcome(request.session_id, result);
    } catch (error) {
      if (active()) {
        await options.refresh();
        if (active()) throw error;
      }
    } finally {
      if (ticket === epoch) busy = false;
    }
  };
}
