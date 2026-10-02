import { useEffect, useState, type CSSProperties } from "react";
import { phaseCopy } from "@/v2/shared/domain/pipeline";
import {
  createTauriOverlayRuntime,
  type OverlayRuntimeSnapshot,
} from "../infrastructure/tauriOverlayRuntime";
import "./overlay-v2.css";

const runtime = createTauriOverlayRuntime();
const waveBars = [10, 17, 25, 15, 31, 20, 28, 13, 23, 17, 10];

export function OverlayV2() {
  const [snapshot, setSnapshot] = useState<OverlayRuntimeSnapshot | null>(null);

  useEffect(() => {
    document.body.classList.add("overlay-mode");
    return () => document.body.classList.remove("overlay-mode");
  }, []);

  useEffect(() => {
    return runtime.subscribe(setSnapshot);
  }, []);

  if (!snapshot || (snapshot.phase === "idle" && !snapshot.preview))
    return null;
  if (!snapshot.settings.overlay_enabled && !snapshot.preview) return null;

  const { phase } = snapshot;
  const isPreview = phase === "idle" && snapshot.preview !== null;
  const settings = isPreview ? snapshot.preview! : snapshot.settings;
  const copy = isPreview ? { label: "Тестовый оверлей" } : phaseCopy[phase];

  return (
    <div className="v2-overlay-stage">
      <div
        className={`v2-overlay is-${phase} ${
          settings.overlay_mini_mode ? "is-mini" : ""
        }`}
        style={{
          opacity: settings.overlay_opacity,
          transform: `scale(${settings.overlay_scale})`,
        }}
        title="Перетащите оверлей мышью"
        onMouseDown={() => void runtime.startDragging()}
      >
        <span className="v2-overlay__glow" aria-hidden="true" />
        <span className="v2-overlay__indicator" aria-hidden="true" />
        {!settings.overlay_mini_mode && (
          <>
            <span className="v2-overlay__copy">
              <strong>{copy.label}</strong>
              <small>
                {isPreview
                  ? "Перетащите в удобное место"
                  : phase === "listening"
                    ? "Говорите — Fono слушает"
                    : "Локальная диктовка"}
              </small>
            </span>
            <span className="v2-overlay__wave" aria-hidden="true">
              {waveBars.map((height, index) => (
                <i
                  key={index}
                  style={
                    {
                      "--delay": `${index * 70}ms`,
                      "--height": `${height}px`,
                    } as CSSProperties
                  }
                />
              ))}
            </span>
          </>
        )}
        {settings.overlay_mini_mode && (
          <span className="v2-overlay__mini-wave" aria-hidden="true">
            {waveBars.slice(3, 8).map((height, index) => (
              <i
                key={index}
                style={
                  {
                    "--delay": `${index * 80}ms`,
                    "--height": `${height}px`,
                  } as CSSProperties
                }
              />
            ))}
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
        {!isPreview && (
          <button
            className="v2-overlay__cancel"
            type="button"
            title="Отменить"
            onMouseDown={(event) => event.stopPropagation()}
            onClick={() => void runtime.cancel()}
          >
            ■
          </button>
        )}
      </div>
    </div>
  );
}
