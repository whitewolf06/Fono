import { reactive } from "vue";
import type { Settings } from "../../../shared/infrastructure/native/ipcTypes";
import type {
  OverlayProcessingChoice,
  OverlayProcessingChoiceRequest,
} from "../../../shared/domain/processing";
import { call } from "../../../shared/infrastructure/native/ipc";

interface Options {
  readConfirmed: () => OverlayProcessingChoice;
  isCurrent: (sessionId: number | null) => boolean;
  commit: (settings: Settings, request: OverlayProcessingChoiceRequest) => void;
  rollback: (
    error: Error,
    confirmed: OverlayProcessingChoice,
    request: OverlayProcessingChoiceRequest,
  ) => void;
  save?: (request: OverlayProcessingChoiceRequest) => Promise<Settings>;
}
interface Entry {
  request: OverlayProcessingChoiceRequest;
  revision: number;
  generation: number;
}

/** Sends immediately, coalesces pending picks, and fences delayed replies. */
export function createOverlayProcessingQueue(options: Options, debounceMs = 0) {
  const state = reactive({ saving: false, pending: false, error: "" });
  const save =
    options.save ??
    ((request: OverlayProcessingChoiceRequest) =>
      call<Settings>("update_overlay_processing_choice", { request }));
  let confirmed = { ...options.readConfirmed() },
    revision = 0,
    generation = 0,
    disposed = false;
  let queued: Entry | null = null,
    active: Promise<void> | null = null,
    failure: Error | null = null,
    timer: ReturnType<typeof setTimeout> | undefined;
  const current = (entry: Entry) =>
    !disposed &&
    entry.generation === generation &&
    entry.revision === revision &&
    options.isCurrent(entry.request.sessionId);

  async function drain() {
    while (!disposed && queued) {
      const entry = queued;
      queued = null;
      if (!options.isCurrent(entry.request.sessionId)) continue;
      state.saving = true;
      try {
        const settings = await save(entry.request);
        if (disposed || entry.generation !== generation) continue;
        confirmed = {
          preset:
            settings.processing_preset ??
            (settings.ai_mode === "format" ? "format" : "clean"),
          targetLanguage: settings.processing_target_language ?? null,
          ...(entry.request.processingEnabled === undefined
            ? {}
            : { processingEnabled: settings.ai_mode !== "off" }),
          ...(entry.request.translationEnabled === undefined
            ? {}
            : { translationEnabled: settings.processing_translation_enabled }),
        };
        if (current(entry)) options.commit(settings, entry.request);
      } catch (error) {
        if (!current(entry)) continue;
        failure = error instanceof Error ? error : new Error(String(error));
        state.error = failure.message;
        options.rollback(failure, { ...confirmed }, entry.request);
        throw failure;
      } finally {
        state.saving = false;
        state.pending = !disposed && queued !== null;
      }
    }
    state.pending = !disposed && queued !== null;
  }
  async function flush() {
    clearTimeout(timer);
    while (!disposed) {
      if (!active && queued) active = drain().finally(() => (active = null));
      if (active) {
        await active;
        continue;
      }
      if (failure) throw failure;
      return;
    }
  }
  function update(request: OverlayProcessingChoiceRequest) {
    if (disposed || !options.isCurrent(request.sessionId)) return;
    if (!active && !queued) confirmed = { ...options.readConfirmed() };
    queued = { request: { ...request }, revision: ++revision, generation };
    state.pending = true;
    state.error = "";
    failure = null;
    clearTimeout(timer);
    if (debounceMs <= 0) void flush().catch(() => {});
    else timer = setTimeout(() => void flush().catch(() => {}), debounceMs);
  }
  function discard() {
    generation++;
    revision++;
    queued = null;
    failure = null;
    confirmed = { ...options.readConfirmed() };
    clearTimeout(timer);
    state.pending = false;
    state.error = "";
  }
  return {
    state,
    update,
    flush,
    discard,
    dispose() {
      disposed = true;
      discard();
    },
  };
}
