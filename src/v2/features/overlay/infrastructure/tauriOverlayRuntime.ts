import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  ipc,
  onOverlayPreview,
  onPipelineStateChange,
  onSettingsChange,
  type OverlayPreview,
} from "@/lib/ipc";
import type { PipelineState, Settings } from "@/lib/types";

export interface OverlayRuntimeSnapshot {
  phase: PipelineState;
  preview: OverlayPreview | null;
  settings: Pick<
    Settings,
    | "overlay_enabled"
    | "overlay_mini_mode"
    | "overlay_opacity"
    | "overlay_scale"
  >;
}

export function createTauriOverlayRuntime() {
  return {
    subscribe(listener: (snapshot: OverlayRuntimeSnapshot) => void) {
      let active = true;
      let phase: PipelineState = "idle";
      let settings: Settings | null = null;
      let receivedPipelineState = false;
      let receivedSettings = false;
      let receivedPreview = false;
      let preview: OverlayPreview | null = null;
      let stopPipeline: (() => void) | undefined;
      let stopSettings: (() => void) | undefined;
      let stopPreview: (() => void) | undefined;
      let stopMove: (() => void) | undefined;
      let moveSaveTimer: number | undefined;

      const notify = () => {
        if (active && settings) listener({ phase, settings, preview });
      };

      const pipelineReady = onPipelineStateChange((nextPhase) => {
        phase = nextPhase;
        receivedPipelineState = true;
        notify();
      }).then((unlisten) => {
        if (active) stopPipeline = unlisten;
        else unlisten();
      });
      const settingsReady = onSettingsChange((nextSettings) => {
        settings = nextSettings;
        receivedSettings = true;
        notify();
      }).then((unlisten) => {
        if (active) stopSettings = unlisten;
        else unlisten();
      });
      const previewReady = onOverlayPreview((nextPreview) => {
        preview = nextPreview;
        receivedPreview = true;
        notify();
      }).then((unlisten) => {
        if (active) stopPreview = unlisten;
        else unlisten();
      });
      void Promise.all([pipelineReady, settingsReady, previewReady])
        .then(() =>
          Promise.all([
            ipc.getPipelineState(),
            ipc.getSettings(),
            ipc.getOverlayPreview(),
          ]),
        )
        .then(([initialPhase, initialSettings, initialPreview]) => {
          if (!active) return;

          if (!receivedSettings) settings = initialSettings;
          if (!receivedPreview) preview = initialPreview;
          if (!receivedPipelineState) phase = initialPhase;
          notify();
        })
        .catch((error: unknown) =>
          console.error("Не удалось загрузить состояние оверлея", error),
        );
      void getCurrentWebviewWindow()
        .onMoved(({ payload: { x, y } }) => {
          if (!active) return;
          if (moveSaveTimer) window.clearTimeout(moveSaveTimer);
          moveSaveTimer = window.setTimeout(() => {
            void ipc
              .saveOverlayPosition(x, y)
              .catch((error: unknown) =>
                console.error("Не удалось сохранить положение оверлея", error),
              );
          }, 500);
        })
        .then((unlisten) => {
          if (active) stopMove = unlisten;
          else unlisten();
        })
        .catch((error: unknown) =>
          console.error("Не удалось отслеживать положение оверлея", error),
        );

      return () => {
        active = false;
        stopPipeline?.();
        stopSettings?.();
        stopPreview?.();
        stopMove?.();
        if (moveSaveTimer) window.clearTimeout(moveSaveTimer);
      };
    },
    startDragging: () => getCurrentWebviewWindow().startDragging(),
    confirm: () => ipc.confirmDictation(),
    cancel: () => ipc.cancelDictation(),
  };
}
