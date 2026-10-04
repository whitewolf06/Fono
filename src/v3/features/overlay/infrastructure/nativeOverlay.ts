import { reactive } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings } from "../../../../lib/types";
import type { Phase, LiveDictation } from "../../../shared/domain/contracts";
import { defaults } from "../../preferences/domain/preferences";
import { preferencesFromNative } from "../../../shared/infrastructure/native/mapping";
import { call, subscribe } from "../../../shared/infrastructure/native/ipc";
import type {
  NativeSnapshot,
  NativeLiveSnapshot,
} from "../../../shared/infrastructure/native/dictation";
import { livePhase } from "../../dictation";
import type {
  PendingDictation,
  OverlayProcessingChoice,
} from "../../../shared/domain/processing";
import { createPendingOverlay } from "./pendingOverlay";
import { createOverlayControls } from "./overlayControls";
import { bindOverlayLayout } from "./overlayLayout";
import {
  createOverlayHydration,
  type NativeOverlayPreview as Preview,
} from "./overlayHydration";
export function createNativeOverlay() {
  document.documentElement.classList.add("native-overlay-document");
  const state = reactive({
    preferences: { ...defaults },
    phase: "idle" as Phase,
    level: 0,
    seconds: 0,
    elapsed: 0,
    error: "",
    source: null as string | null,
    live: null as LiveDictation | null,
    pending: null as PendingDictation | null,
    preview: null as Preview | null,
    sessionId: null as number | null,
    processingChoice: null as OverlayProcessingChoice | null,
    helpOpen: false,
  });
  let settings: Settings | null = null,
    disposed = false,
    polling = false,
    started = Date.now(),
    operation = -1;
  const pending = createPendingOverlay(state, () => disposed);
  const releases: (() => void)[] = [];
  const readiness: Promise<unknown>[] = [];
  let moveTimer: ReturnType<typeof setTimeout> | undefined;
  const fail = (error: unknown) => {
    if (disposed) return;
    state.error = error instanceof Error ? error.message : String(error);
  };
  const controls = createOverlayControls(
    state,
    () => settings,
    (s) => {
      settings = s;
      applySettings();
    },
    () => disposed,
  );
  const releaseLayout = bindOverlayLayout(
    state,
    () => controls.state.error,
    fail,
  );
  function applySettings() {
    if (settings) state.preferences = preferencesFromNative(settings, []);
    if (state.preferences.dictationMode !== "live") state.live = null;
    if (state.preview && state.phase === "idle")
      Object.assign(state.preferences, {
        overlayEnabled: true,
        overlayScale: state.preview.overlay_scale * 100,
        overlayOpacity: state.preview.overlay_opacity * 100,
        overlayCompact: state.preview.overlay_mini_mode,
      });
  }
  function bind<T>(channel: string, listener: (v: T) => void) {
    const ready = subscribe<T>(channel, (value) => {
      if (!disposed) listener(value);
    })
      .then((release) => (disposed ? release() : releases.push(release)))
      .catch(fail);
    readiness.push(ready);
  }
  const hydration = createOverlayHydration({
    disposed: () => disposed,
    settings: (s) => {
      settings = s;
      applySettings();
    },
    preview: (p) => {
      state.preview = p;
      applySettings();
    },
    pending,
  });
  bind<Settings>("settings-changed", hydration.settingsChanged);
  function applyLive(v: NativeLiveSnapshot | null) {
    if (!v || (settings && settings.dictation_mode !== "live")) return;
    if (state.live && Number(v.session_id) < Number(state.live.sessionId))
      return;
    if (
      state.live?.sessionId === v.session_id &&
      state.live.revision >= v.revision
    ) {
      if (state.live.revision === v.revision) {
        state.elapsed = Math.floor(v.elapsed_ms / 1000);
        state.level = Math.min(1, Math.max(0, v.audio_level));
      }
      return;
    }
    state.live = {
      sessionId: v.session_id,
      revision: v.revision,
      committedText: v.committed_text,
      draftText: v.draft_text,
      pendingText: v.pending_text,
      insertionState: v.insertion_state,
      phase: v.phase,
      lagMs: v.lag_ms,
      warning: v.warning || undefined,
    };
    state.phase = livePhase(state.live);
    state.elapsed = Math.floor(v.elapsed_ms / 1000);
    state.level = Math.min(1, Math.max(0, v.audio_level));
    state.source = v.source;
  }
  bind<NativeLiveSnapshot>("dictation-live", applyLive);
  bind<PendingDictation | null>("pending-dictation", pending.apply);
  bind<Preview | null>("overlay-preview", hydration.previewChanged);
  bind<{ remaining_ms: number; speaking: boolean }>(
    "wake-dictation-countdown",
    (v) => {
      state.seconds = v.speaking ? 0 : Math.ceil(v.remaining_ms / 1000);
    },
  );
  void hydration.load(readiness).catch(fail);
  const window = getCurrentWindow();
  void window
    .onMoved(({ payload: { x, y } }) => {
      clearTimeout(moveTimer);
      moveTimer = setTimeout(() => {
        void call("save_overlay_position", { x, y }).catch(fail);
      }, 500);
    })
    .then((release) => (disposed ? release() : releases.push(release)))
    .catch(fail);
  const timer = setInterval(async () => {
    if (disposed || polling) return;
    polling = true;
    const ticket = pending.stamp();
    try {
      const [v, live, dictation] = await Promise.all([
        call<NativeSnapshot>("get_desktop_snapshot"),
        call<NativeLiveSnapshot | null>("get_live_dictation"),
        call<PendingDictation | null>("get_pending_dictation"),
      ]);
      if (disposed) return;
      if (v.operation_id > 0 && v.operation_id < operation) return;
      if (!pending.current(ticket)) return;
      state.sessionId = v.operation_id > 0 ? v.operation_id : null;
      pending.observeOperation(v.operation_id);
      pending.apply(dictation, ticket);
      if (state.pending) {
        operation = Math.max(operation, state.pending.sessionId);
        applySettings();
        return;
      }
      applyLive(live);
      if (state.live && state.preferences.dictationMode === "live") {
        applySettings();
        return;
      }
      if (v.operation_id > 0 && operation !== v.operation_id) {
        operation = v.operation_id;
        state.error = "";
        started = Date.now();
        state.seconds = 0;
      }
      state.source = v.source;
      state.phase =
        v.state === "injecting"
          ? "processing"
          : v.state === "listening" && state.seconds > 0
            ? "silence"
            : v.state;
      state.level = Math.min(1, v.level * 8);
      state.elapsed = Math.floor((Date.now() - started) / 1000);
      applySettings();
    } catch (error) {
      if (pending.current(ticket)) fail(error);
    } finally {
      polling = false;
    }
  }, 120);
  return {
    state,
    processing: controls.state,
    chooseProcessing: (choice: OverlayProcessingChoice) => {
      state.error = "";
      controls.choose(choice);
    },
    setHelp: (open: boolean) => {
      state.helpOpen = open;
    },
    drag: () => window.startDragging().catch(fail),
    resume: () => call("resume_live_insertion").catch(fail),
    resolve: (request: Parameters<typeof pending.resolve>[0]) =>
      controls.resolve(request, pending.resolve).catch(fail),
    copy: pending.copy,
    finish: () => {
      const sessionId = state.sessionId;
      const source = state.source;
      return controls
        .finish(() =>
          state.preview && state.phase === "idle"
            ? call("dismiss_dictation_overlay")
            : sessionId !== null && sessionId > 0
              ? call(
                  source === "wake_word"
                    ? "confirm_dictation"
                    : "stop_dictation",
                  { sessionId },
                )
              : Promise.resolve(),
        )
        .catch(fail);
    },
    cancel: () => {
      controls.cancel();
      return (
        state.pending
          ? pending.resolve({
              sessionId: state.pending.sessionId,
              action: "cancel",
            })
          : ["idle", "done", "error", "cancelled"].includes(state.phase)
            ? call("dismiss_dictation_overlay")
            : state.sessionId !== null && state.sessionId > 0
              ? call("cancel_dictation", { sessionId: state.sessionId })
              : Promise.resolve()
      ).catch(fail);
    },
    dispose() {
      disposed = true;
      clearInterval(timer);
      clearTimeout(moveTimer);
      controls.dispose();
      releaseLayout();
      releases.forEach((stop) => stop());
    },
  };
}
