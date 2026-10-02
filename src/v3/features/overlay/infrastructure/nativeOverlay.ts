import { reactive } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings } from "../../../../lib/types";
import type { Phase } from "../../../shared/domain/contracts";
import { defaults } from "../../preferences/domain/preferences";
import { preferencesFromNative } from "../../../shared/infrastructure/native/mapping";
import { call, subscribe } from "../../../shared/infrastructure/native/ipc";
import type { NativeSnapshot } from "../../../shared/infrastructure/native/dictation";
interface Preview {
  overlay_scale: number;
  overlay_opacity: number;
  overlay_mini_mode: boolean;
}
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
    preview: null as Preview | null,
  });
  let settings: Settings | null = null,
    disposed = false,
    polling = false,
    started = Date.now(),
    operation = -1;
  const releases: (() => void)[] = [];
  let moveTimer: ReturnType<typeof setTimeout> | undefined;
  const fail = (error: unknown) => {
    state.error = error instanceof Error ? error.message : String(error);
  };
  function applySettings() {
    if (settings) state.preferences = preferencesFromNative(settings, []);
    if (state.preview && state.phase === "idle")
      Object.assign(state.preferences, {
        overlayEnabled: true,
        overlayScale: state.preview.overlay_scale * 100,
        overlayOpacity: state.preview.overlay_opacity * 100,
        overlayCompact: state.preview.overlay_mini_mode,
      });
  }
  function bind<T>(channel: string, listener: (v: T) => void) {
    void subscribe(channel, listener)
      .then((release) => (disposed ? release() : releases.push(release)))
      .catch(fail);
  }
  bind<Settings>("settings-changed", (s) => {
    settings = s;
    applySettings();
  });
  bind<Preview | null>("overlay-preview", (p) => {
    state.preview = p;
    applySettings();
  });
  bind<{ remaining_ms: number; speaking: boolean }>(
    "wake-dictation-countdown",
    (v) => {
      state.seconds = v.speaking ? 0 : Math.ceil(v.remaining_ms / 1000);
    },
  );
  void Promise.all([
    call<Settings>("get_settings"),
    call<Preview | null>("get_overlay_preview"),
  ])
    .then(([s, p]) => {
      settings = s;
      state.preview = p;
      applySettings();
    })
    .catch(fail);
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
    try {
      const v = await call<NativeSnapshot>("get_desktop_snapshot");
      if (operation !== v.operation_id) {
        operation = v.operation_id;
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
      fail(error);
    } finally {
      polling = false;
    }
  }, 120);
  return {
    state,
    drag: () => window.startDragging().catch(fail),
    finish: () =>
      (state.preview && state.phase === "idle"
        ? call("hide_overlay_preview")
        : call(
            state.source === "wake_word"
              ? "confirm_dictation"
              : "stop_dictation",
          )
      ).catch(fail),
    cancel: () =>
      (state.phase === "idle"
        ? call("hide_overlay_preview")
        : call("cancel_dictation")
      ).catch(fail),
    dispose() {
      disposed = true;
      clearInterval(timer);
      clearTimeout(moveTimer);
      releases.forEach((stop) => stop());
    },
  };
}
