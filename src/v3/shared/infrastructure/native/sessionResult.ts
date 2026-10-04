import type { Dictation } from "../../domain/contracts";
import type { NativeContext } from "./context";
import type { NativeResult } from "./dictationContracts";

/** Session output is independent from the optional archive and local draft. */
export function createNativeResultObserver(
  ctx: NativeContext,
  retiredSessions: Set<string>,
  changed: () => void,
) {
  const acceptedResults = new Map<string, Set<string>>();
  return (result: NativeResult) => {
    const { state } = ctx;
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
    changed();
    void ctx.readHistory().catch(ctx.report);
  };
}
