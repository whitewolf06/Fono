import type { Settings } from "../../../shared/infrastructure/native/ipcTypes";
import type { PendingDictation } from "../../../shared/domain/processing";
import { call } from "../../../shared/infrastructure/native/ipc";

export interface NativeOverlayPreview {
  overlay_scale: number;
  overlay_opacity: number;
  overlay_mini_mode: boolean;
}
interface Options {
  disposed: () => boolean;
  settings: (settings: Settings) => void;
  preview: (preview: NativeOverlayPreview | null) => void;
  pending: {
    stamp: () => number;
    apply: (value: PendingDictation | null, ticket?: number) => void;
  };
}
/** Subscribe first. Events received during reads win over their older replies. */
export function createOverlayHydration(options: Options) {
  let settingsEpoch = 0,
    previewEpoch = 0;
  return {
    settingsChanged(value: Settings) {
      settingsEpoch++;
      options.settings(value);
    },
    previewChanged(value: NativeOverlayPreview | null) {
      previewEpoch++;
      options.preview(value);
    },
    async load(readiness: Promise<unknown>[]) {
      await Promise.all(readiness);
      if (options.disposed()) return;
      const settingsTicket = settingsEpoch,
        previewTicket = previewEpoch,
        pendingTicket = options.pending.stamp();
      const [settings, preview, pending] = await Promise.all([
        call<Settings>("get_settings"),
        call<NativeOverlayPreview | null>("get_overlay_preview"),
        call<PendingDictation | null>("get_pending_dictation"),
      ]);
      if (options.disposed()) return;
      if (settingsEpoch === settingsTicket) options.settings(settings);
      if (previewEpoch === previewTicket) options.preview(preview);
      options.pending.apply(pending, pendingTicket);
    },
  };
}
