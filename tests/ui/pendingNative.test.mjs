import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

let server,
  nativeDictation,
  createPendingOverlay,
  isBackendEventV1,
  defaults,
  invoke,
  calls;
before(async () => {
  globalThis.window = {
    __TAURI_INTERNALS__: { invoke: (...args) => invoke(...args) },
  };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ nativeDictation } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/dictation.ts",
  ));
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  ({ createPendingOverlay } = await server.ssrLoadModule(
    "/src/v3/features/overlay/infrastructure/pendingOverlay.ts",
  ));
  ({ isBackendEventV1 } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/backendEvents.ts",
  ));
});
after(async () => {
  await server.close();
  delete globalThis.window;
});
beforeEach(() => {
  calls = [];
  invoke = async (name, args) => {
    calls.push([name, args]);
    if (name === "get_desktop_snapshot") return desktop();
    if (name === "get_live_dictation" || name === "get_pending_dictation")
      return null;
  };
});

function desktop(operation = 0, state = "idle", last = null) {
  return {
    operation_id: operation,
    state,
    last,
    source: operation ? "hotkey" : null,
    level: 0,
  };
}
function pending(sessionId = 1, phase = "awaiting_action") {
  return {
    sessionId,
    phase,
    originalText: "original",
    resultText: null,
    createdAt: "2026-10-04T10:00:00Z",
    preset: "clean",
    targetLanguage: null,
    processingEnabled: true,
    source: "hotkey",
    error: null,
    insertionBlocked: false,
  };
}
function harness() {
  const state = {
    preferences: { ...defaults },
    phase: "idle",
    last: {
      entry: null,
      variant: "result",
      draft: "",
      edited: false,
      undo: null,
    },
    elapsed: 0,
    audioLevel: 0,
    error: "",
    testSignal: 0,
  };
  const errors = [];
  const adapter = nativeDictation({
    state,
    readHistory: async () => {},
    report: (error) => errors.push(error),
  });
  return { state, adapter, errors };
}
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function result(id = "entry-1", text = "processed") {
  return {
    id,
    text,
    original_text: "original",
    created_at: "2026-10-04T10:00:00Z",
    audio_secs: 4,
  };
}

test("explicit no-speech completion is neutral and preserves the previous result without fabricating a new one", () => {
  const { state, adapter } = harness();
  adapter.snapshot(desktop(8, "transcribing", result("old")));
  const previous = state.last.entry;
  adapter.outcome({ operation_id: 8, outcome: "no_speech" });
  assert.equal(state.phase, "idle");
  assert.equal(state.dictationOutcome, "no_speech");
  assert.equal(state.error, "");
  assert.equal(state.last.entry, previous);
  assert.equal(state.last.entry.id, "old");
});

test("hidden-window hydration restores the latest no-speech outcome and an older outcome cannot alter a successor", () => {
  const { state, adapter } = harness();
  adapter.snapshot({
    ...desktop(),
    outcome: { operation_id: 9, outcome: "no_speech" },
  });
  assert.equal(state.dictationOutcome, "no_speech");
  adapter.snapshot({
    ...desktop(10, "listening"),
    outcome: { operation_id: 9, outcome: "cancelled" },
  });
  assert.equal(state.phase, "listening");
  assert.equal(state.dictationOutcome, undefined);
  adapter.outcome({ operation_id: 9, outcome: "superseded" });
  assert.equal(state.phase, "listening");
  assert.equal(state.dictationOutcome, undefined);
});

test("a successor first observed during transcription retains its new pending text after an earlier cancellation", () => {
  const { state, adapter } = harness();
  adapter.snapshot(desktop(8, "processing"));
  adapter.outcome({ operation_id: 8, outcome: "cancelled" });
  assert.equal(state.dictationOutcome, "cancelled");

  // The main window missed listening while hidden and only saw this snapshot.
  adapter.snapshot(desktop(9, "transcribing"));
  assert.equal(state.dictationOutcome, undefined);
  assert.equal(state.phase, "transcribing");

  const next = pending(9);
  adapter.pending(next);
  adapter.outcome({ operation_id: 9, outcome: "deferred" });
  assert.equal(state.pendingDictation.sessionId, 9);
  assert.equal(state.pendingDictation.originalText, next.originalText);
  assert.equal(state.phase, "awaiting_action");
  assert.equal(state.dictationOutcome, "deferred");
});

test("a successor first observed during transcription clears the previous session's pending error", () => {
  const { state, adapter } = harness();
  adapter.pending({ ...pending(8), error: "Previous processing failure" });
  adapter.snapshot(desktop(9, "transcribing"));
  assert.equal(state.pendingDictation, null);
  assert.equal(state.error, "");
  assert.equal(state.phase, "transcribing");
});

test("an idle terminal snapshot retires an adjacent pending getter reply before the pending early-return", async () => {
  for (const kind of ["cancelled", "no_speech", "superseded"]) {
    const { state, adapter } = harness();
    invoke = async (name) =>
      name === "get_pending_dictation"
        ? pending(9)
        : name === "get_desktop_snapshot"
          ? { ...desktop(), outcome: { operation_id: 9, outcome: kind } }
          : null;
    await adapter.refresh();
    assert.equal(state.pendingDictation, null);
    assert.equal(state.dictationOutcome, kind);
    assert.equal(state.phase, kind === "cancelled" ? "cancelled" : "idle");
  }
});

test("a previous outcome arriving during a new start cannot label the new recording", async () => {
  const { state, adapter } = harness();
  adapter.snapshot(desktop(8));
  const starting = deferred();
  invoke = async (name) =>
    name === "start_dictation" ? starting.promise : undefined;
  const start = adapter.port.start();
  adapter.outcome({ operation_id: 8, outcome: "no_speech" });
  assert.equal(state.dictationOutcome, undefined);
  adapter.snapshot(desktop(9, "listening"));
  starting.resolve();
  await start;
  assert.equal(state.phase, "listening");
});

test("stop replies distinguish no speech and ignore superseded replies after a successor begins", async () => {
  const { state, adapter } = harness();
  adapter.snapshot(desktop(8, "transcribing"));
  invoke = async (name) =>
    name === "stop_dictation" ? { kind: "no_speech" } : undefined;
  await adapter.port.finish();
  assert.equal(state.dictationOutcome, "no_speech");
  assert.equal(state.phase, "idle");
  adapter.snapshot(desktop(9, "transcribing"));
  const stopped = deferred();
  invoke = async (name) =>
    name === "stop_dictation" ? stopped.promise : undefined;
  const finish = adapter.port.finish();
  adapter.snapshot(desktop(10, "listening"));
  stopped.resolve({ kind: "superseded" });
  await finish;
  assert.equal(state.phase, "listening");
  assert.equal(state.dictationOutcome, undefined);
});

test("pending overlay no-speech/cancelled/superseded replies never report successful text completion", async () => {
  for (const kind of ["no_speech", "cancelled", "superseded"]) {
    const state = {
      pending: null,
      phase: "idle",
      source: null,
      level: 0,
      error: "",
    };
    const adapter = createPendingOverlay(state, () => false);
    adapter.apply(pending(11));
    invoke = async (name) =>
      name === "resolve_pending_dictation" ? { kind } : null;
    await adapter.resolve({ sessionId: 11, action: "insert_raw" });
    assert.equal(state.dictationOutcome, kind);
    assert.equal(state.pending, null);
    assert.equal(state.phase, kind === "cancelled" ? "cancelled" : "idle");
  }
});

test("dictation outcome events require a positive integer operation and a known nontext outcome", () => {
  const event = (payload) => ({
    schema_version: 1,
    kind: "dictation_outcome",
    payload,
  });
  assert.equal(
    isBackendEventV1(event({ operation_id: 3, outcome: "no_speech" })),
    true,
  );
  for (const payload of [
    null,
    { operation_id: 0, outcome: "no_speech" },
    { operation_id: 2.5, outcome: "cancelled" },
    { operation_id: 3, outcome: "future" },
  ])
    assert.equal(isBackendEventV1(event(payload)), false);
});

test("terminal cancellation fences same-session late outcomes and pending events in both native adapters", () => {
  const { state, adapter } = harness();
  adapter.snapshot(desktop(8, "processing"));
  adapter.outcome({ operation_id: 8, outcome: "cancelled" });
  adapter.outcome({ operation_id: 8, outcome: "completed" });
  adapter.pending(pending(8));
  assert.equal(state.dictationOutcome, "cancelled");
  assert.equal(state.pendingDictation, undefined);
  assert.equal(state.phase, "cancelled");
  adapter.pending(pending(9));
  adapter.outcome({ operation_id: 8, outcome: "no_speech" });
  assert.equal(state.pendingDictation.sessionId, 9);
  assert.equal(state.dictationOutcome, undefined);
  adapter.outcome({ operation_id: 9, outcome: "deferred" });
  assert.equal(state.dictationOutcome, "deferred");
  const overlayState = {
    pending: null,
    phase: "idle",
    source: null,
    level: 0,
    error: "",
  };
  const overlay = createPendingOverlay(overlayState, () => false);
  overlay.retire(8);
  overlay.apply(pending(8));
  overlay.apply(pending(7));
  assert.equal(overlayState.pending, null);
  overlay.apply(pending(9));
  assert.equal(overlayState.pending.sessionId, 9);
});

test("initial hydration restores a pending session without requiring an event", async () => {
  const { state, adapter } = harness();
  invoke = async (name) =>
    name === "get_desktop_snapshot"
      ? desktop(7, "awaiting_action", result())
      : name === "get_pending_dictation"
        ? pending(7)
        : null;
  await adapter.refresh();
  assert.equal(state.pendingDictation.sessionId, 7);
  assert.equal(state.phase, "awaiting_action");
  assert.equal(state.last.entry.id, "entry-1");
});
test("a stale nullable poll cannot erase a newer pending event", async () => {
  const { state, adapter } = harness(),
    response = deferred();
  invoke = async (name) =>
    name === "get_pending_dictation"
      ? response.promise
      : name === "get_desktop_snapshot"
        ? desktop()
        : null;
  const poll = adapter.refresh();
  adapter.pending(pending(2));
  response.resolve(null);
  await poll;
  assert.equal(state.pendingDictation.sessionId, 2);
  assert.equal(state.phase, "awaiting_action");
});
test("a stale non-null poll cannot restore awaiting after a processing event", async () => {
  const { state, adapter } = harness(),
    response = deferred();
  adapter.pending(pending(2));
  invoke = async (name) =>
    name === "get_pending_dictation"
      ? response.promise
      : name === "get_desktop_snapshot"
        ? desktop(2, "awaiting_action")
        : null;
  const poll = adapter.refresh();
  adapter.pending(pending(2, "processing"));
  response.resolve(pending(2));
  await poll;
  assert.equal(state.pendingDictation.phase, "processing");
  assert.equal(state.phase, "processing");
});
test("latest refresh generation owns state even when an older call resolves last", async () => {
  const { state, adapter } = harness(),
    first = deferred();
  let count = 0;
  invoke = async (name) =>
    name === "get_pending_dictation"
      ? ++count === 1
        ? first.promise
        : pending(3)
      : name === "get_desktop_snapshot"
        ? desktop(3, "awaiting_action")
        : null;
  const old = adapter.refresh();
  await adapter.refresh();
  first.resolve(null);
  await old;
  assert.equal(state.pendingDictation.sessionId, 3);
});
test("nullable event confirms native state and cannot blindly dismiss another session", async () => {
  const { state, adapter } = harness();
  adapter.pending(pending(2));
  invoke = async (name) =>
    name === "get_pending_dictation"
      ? pending(2)
      : name === "get_desktop_snapshot"
        ? desktop(2, "awaiting_action")
        : null;
  adapter.pending(null);
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(state.pendingDictation.sessionId, 2);
});
test("resolve ignores Transcript metadata and refreshes canonical LastDictation", async () => {
  const { state, adapter } = harness();
  adapter.pending(pending(1));
  const canonical = result();
  invoke = async (name, args) => {
    calls.push([name, args]);
    if (name === "resolve_pending_dictation")
      return { text: "processed", device: "CPU", audio_secs: 4 };
    if (name === "get_desktop_snapshot") return desktop(0, "idle", canonical);
    return null;
  };
  await adapter.port.resolvePending({
    sessionId: 1,
    action: "process_and_insert",
    preset: "task",
    targetLanguage: "en",
  });
  assert.equal(state.last.entry.id, "entry-1");
  assert.equal(state.last.entry.original, "original");
  assert.equal(state.last.entry.createdAt, canonical.created_at);
  assert.equal(state.pendingDictation, null);
  assert.equal(state.phase, "done");
  assert.equal(calls[0][1].request.targetLanguage, "en");
});
test("late old action response cannot overwrite the new pending session", async () => {
  const { state, adapter } = harness(),
    action = deferred();
  adapter.pending(pending(1));
  invoke = async (name, args) => {
    calls.push([name, args]);
    if (name === "resolve_pending_dictation") return action.promise;
    throw new Error("Stale action must not refresh");
  };
  const resolving = adapter.port.resolvePending({
    sessionId: 1,
    action: "insert_raw",
  });
  adapter.pending(pending(2));
  action.resolve({ text: "old" });
  await resolving;
  assert.equal(state.pendingDictation.sessionId, 2);
  assert.equal(state.last.entry, null);
  assert.equal(calls.length, 1);
});
test("repeated action in the same window issues only one processing command", async () => {
  const { adapter } = harness(),
    action = deferred();
  adapter.pending(pending(1));
  let commands = 0;
  invoke = async (name) => {
    if (name === "resolve_pending_dictation") {
      commands++;
      return action.promise;
    }
    if (name === "get_desktop_snapshot") return desktop();
    return null;
  };
  const request = { sessionId: 1, action: "process_and_insert" };
  const first = adapter.port.resolvePending(request);
  await adapter.port.resolvePending(request);
  assert.equal(commands, 1);
  action.resolve(null);
  await first;
});
test("Stop remains available for both hold and toggle global shortcut captures", async () => {
  const { state, adapter } = harness();
  state.recordingSource = "hotkey";
  for (const mode of ["hold", "toggle"]) {
    state.preferences.hotkeyMode = mode;
    state.phase = "listening";
    await adapter.port.finish();
  }
  assert.deepEqual(
    calls.map(([name]) => name),
    ["stop_dictation", "stop_dictation"],
  );
});

test("late processing failure after explicit cancellation is not surfaced again", async () => {
  const { state, adapter } = harness(),
    processing = deferred();
  adapter.pending(pending(1));
  invoke = async (name, args) => {
    if (
      name === "resolve_pending_dictation" &&
      args.request.action !== "cancel"
    )
      return processing.promise;
    if (name === "get_desktop_snapshot") return desktop();
    return null;
  };
  const work = adapter.port.resolvePending({
    sessionId: 1,
    action: "process_and_insert",
  });
  await adapter.port.resolvePending({ sessionId: 1, action: "cancel" });
  assert.equal(state.pendingDictation, null);
  processing.reject(new Error("Request cancelled"));
  await assert.doesNotReject(work);
});
