import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ipc, onPipelineStateChange, onSettingsChange } from "@/lib/ipc";
import type { PipelineState, Settings } from "@/lib/types";

const baseSize = { width: 286, height: 88 };

export interface OverlayRuntimeSnapshot {
  phase: PipelineState;
  settings: Pick<
    Settings,
    "overlay_mini_mode" | "overlay_opacity" | "overlay_scale"
  >;
}

export function createTauriOverlayRuntime() {
  return {
    getSnapshot: async (): Promise<OverlayRuntimeSnapshot> => {
      const [phase, settings] = await Promise.all([
        ipc.getPipelineState(),
        ipc.getSettings(),
      ]);
      return { phase, settings };
    },
    subscribe(listener: (snapshot: OverlayRuntimeSnapshot) => void) {
      let active = true;
      let phase: PipelineState = "idle";
      let settings: Settings | null = null;
      let receivedPipelineState = false;
      let stopPipeline: (() => void) | undefined;
      let stopSettings: (() => void) | undefined;
      let stopMove: (() => void) | undefined;
      let moveSaveTimer: number | undefined;

      const notify = () => {
        if (active && settings) listener({ phase, settings });
      };

      void onPipelineStateChange((nextPhase) => {
        phase = nextPhase;
        receivedPipelineState = true;
        notify();
      }).then((unlisten) => {
        if (active) stopPipeline = unlisten;
        else unlisten();
      });
      void onSettingsChange((nextSettings) => {
        settings = nextSettings;
        notify();
      }).then((unlisten) => {
        if (active) stopSettings = unlisten;
        else unlisten();
      });
      void Promise.all([ipc.getPipelineState(), ipc.getSettings()]).then(
        ([initialPhase, initialSettings]) => {
          if (!active) return;

          settings ??= initialSettings;
          if (!receivedPipelineState) phase = initialPhase;
          notify();
        },
      );
      void getCurrentWebviewWindow()
        .onMoved(({ payload: { x, y } }) => {
          if (moveSaveTimer) window.clearTimeout(moveSaveTimer);
          moveSaveTimer = window.setTimeout(() => {
            void ipc.saveOverlayPosition(x, y);
          }, 500);
        })
        .then((unlisten) => {
          if (active) stopMove = unlisten;
          else unlisten();
        });

      return () => {
        active = false;
        stopPipeline?.();
        stopSettings?.();
        stopMove?.();
        if (moveSaveTimer) window.clearTimeout(moveSaveTimer);
      };
    },
    async setScale(scale: number) {
      const window = getCurrentWebviewWindow();
      await window.setSize(
        new LogicalSize(
          Math.round(baseSize.width * scale),
          Math.round(baseSize.height * scale),
        ),
      );
    },
    startDragging: () => getCurrentWebviewWindow().startDragging(),
    confirm: () => ipc.confirmDictation(),
    cancel: () => ipc.cancelDictation(),
  };
}
