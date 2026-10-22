import { reactive } from "vue";
import type {
  WakeSampleRequest,
  WakeSamplesBackend,
  WakeSamplesPort,
  WakeSamplesState,
} from "../domain/samples";
import { message } from "../../../shared/domain/messages";

export function createWakeSamples(
  backend: WakeSamplesBackend,
): WakeSamplesPort {
  const state = reactive<WakeSamplesState>({
    samples: [],
    phase: "idle",
    remainingMs: 0,
    error: "",
    audio: null,
    testAvailable: false,
    testingId: null,
    testResults: {},
  });
  let generation = 0;
  let audioGeneration = 0;
  let session: number | null = null;
  let pendingStart: Promise<{ sessionId: number; durationMs: number }> | null =
    null;
  let finishWork: Promise<void> | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;
  let disposed = false;
  let listRevision = 0;
  let releaseAudio: (() => void) | undefined;
  let testGeneration = 0;
  const clearAudio = () => {
    ++audioGeneration;
    state.audio = null;
    releaseAudio?.();
    releaseAudio = undefined;
  };
  const report = (e: unknown) => {
    if (!disposed)
      state.error =
        e instanceof Error ? e.message : message("common.actionFailed");
  };
  const stopTimer = () => {
    clearInterval(timer);
    timer = undefined;
    state.remainingMs = 0;
  };
  async function load() {
    const revision = ++listRevision;
    try {
      const [samples, available] = await Promise.all([
        backend.list(),
        backend.testAvailable(),
      ]);
      if (!disposed && revision === listRevision) {
        state.samples = samples;
        state.testAvailable = available;
      }
    } catch (e) {
      report(e);
    }
  }
  async function finish() {
    if (finishWork) return finishWork;
    if (disposed || session === null || state.phase !== "recording") return;
    const id = session;
    const token = generation;
    stopTimer();
    state.phase = "saving";
    finishWork = (async () => {
      try {
        const sample = await backend.finish(id);
        if (disposed || token !== generation) return;
        ++listRevision;
        state.samples = [
          sample,
          ...state.samples.filter((s) => s.id !== sample.id),
        ];
      } catch (e) {
        if (token === generation) report(e);
      } finally {
        if (token === generation) {
          session = null;
          state.phase = "idle";
        }
      }
    })();
    try {
      await finishWork;
    } finally {
      finishWork = null;
    }
  }
  async function record(request: WakeSampleRequest) {
    if (disposed || state.phase !== "idle" || state.testingId !== null) return;
    state.error = "";
    if (!request.consent) {
      state.error = message("wakeSamples.consentRequired");
      return;
    }
    if (request.phraseId === "privet_lira" && request.language !== "ru") {
      state.error = message("wakeSamples.errorPhrase");
      return;
    }
    const token = ++generation;
    clearAudio();
    state.phase = "starting";
    const started = backend.start({ ...request });
    pendingStart = started;
    try {
      const capture = await started;
      if (disposed || token !== generation) return;
      session = capture.sessionId;
      state.remainingMs = capture.durationMs;
      state.phase = "recording";
      const end = Date.now() + capture.durationMs;
      timer = setInterval(() => {
        if (token !== generation || disposed) return;
        state.remainingMs = Math.max(0, end - Date.now());
        if (state.remainingMs === 0) void finish();
      }, 100);
    } catch (e) {
      if (token === generation) {
        report(e);
        state.phase = "idle";
      }
    } finally {
      if (pendingStart === started) pendingStart = null;
    }
  }
  async function cancel() {
    ++testGeneration;
    clearAudio();
    const starting = pendingStart;
    const id = session;
    if (!starting && id === null) return;
    const token = ++generation;
    stopTimer();
    state.phase = "cancelling";
    session = null;
    try {
      const target = id ?? (await starting)?.sessionId;
      if (target !== undefined) await backend.cancel(target);
      await finishWork;
    } catch (e) {
      report(e);
    } finally {
      if (token === generation) {
        pendingStart = null;
        state.phase = "idle";
      }
    }
  }
  return {
    state,
    load,
    record,
    finish,
    cancel,
    async test(id) {
      const sample = state.samples.find((sample) => sample.id === id);
      if (
        disposed ||
        state.phase !== "idle" ||
        state.testingId !== null ||
        !state.testAvailable ||
        sample?.phraseId !== "privet_lira"
      )
        return;
      const token = ++testGeneration;
      state.testingId = id;
      state.error = "";
      delete state.testResults[id];
      clearAudio();
      try {
        const result = await backend.test(id);
        if (
          !disposed &&
          token === testGeneration &&
          state.samples.some(
            (row) => row.id === id && row.sha256 === sample.sha256,
          )
        )
          state.testResults[id] = result;
      } catch (e) {
        if (token === testGeneration) report(e);
      } finally {
        state.testingId = null;
      }
    },
    async listen(id) {
      clearAudio();
      const token = audioGeneration;
      state.error = "";
      try {
        const preview = await backend.audio(id);
        if (
          !disposed &&
          token === audioGeneration &&
          state.samples.some((s) => s.id === id)
        ) {
          state.audio = { id, url: preview.url };
          releaseAudio = preview.release;
        } else preview.release();
      } catch (e) {
        if (token === audioGeneration) report(e);
      }
    },
    async remove(id) {
      if (state.testingId !== null) return;
      state.error = "";
      clearAudio();
      try {
        await backend.remove(id);
        ++listRevision;
        if (!disposed) {
          state.samples = state.samples.filter((s) => s.id !== id);
          delete state.testResults[id];
        }
      } catch (e) {
        report(e);
      }
    },
    dispose() {
      disposed = true;
      clearAudio();
      void cancel();
      stopTimer();
    },
  };
}
