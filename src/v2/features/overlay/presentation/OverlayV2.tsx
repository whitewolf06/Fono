import { useEffect, useState } from "react";
import { phaseCopy } from "@/v2/shared/domain/pipeline";
import {
  createTauriOverlayRuntime,
  type OverlayRuntimeSnapshot,
} from "../infrastructure/tauriOverlayRuntime";
import "./overlay-v2.css";

const runtime = createTauriOverlayRuntime();

export function OverlayV2() {
  const [snapshot, setSnapshot] = useState<OverlayRuntimeSnapshot | null>(null);

  useEffect(() => {
    void runtime.getSnapshot().then(setSnapshot);
    return runtime.subscribe(setSnapshot);
  }, []);

  useEffect(() => {
    if (snapshot) void runtime.setScale(snapshot.settings.overlay_scale);
  }, [snapshot?.settings.overlay_scale]);

  if (!snapshot || snapshot.phase === "idle") return null;

  const { phase, settings } = snapshot;
  const copy = phaseCopy[phase];

  return (
    <div
      className="v2-overlay"
      style={{ opacity: settings.overlay_opacity }}
      title="Перетащите оверлей мышью"
      onMouseDown={() => void runtime.startDragging()}
    >
      <span className={`v2-overlay__indicator is-${phase}`} />
      {!settings.overlay_mini_mode && (
        <span className="v2-overlay__copy">
          <strong>{copy.label}</strong>
          <small>диктовка</small>
        </span>
      )}
      {phase === "listening" && (
        <button
          className="v2-overlay__confirm"
          type="button"
          title="Завершить запись"
          onMouseDown={(event) => event.stopPropagation()}
          onClick={() => void runtime.confirm()}
        >
          ✓
        </button>
      )}
      <button
        className="v2-overlay__cancel"
        type="button"
        title="Отменить"
        onMouseDown={(event) => event.stopPropagation()}
        onClick={() => void runtime.cancel()}
      >
        ■
      </button>
    </div>
  );
}
