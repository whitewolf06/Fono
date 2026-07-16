import { useEffect, useState, type CSSProperties } from "react";
import { phaseCopy } from "@/v2/shared/domain/pipeline";
import { MicIcon } from "@/v2/shared/presentation/components/MicIcon";
import { useDictationDashboard } from "../application/useDictationDashboard";
import type { DictationRuntime } from "../application/dictationRuntime";
import { CanvasVoiceWave } from "./CanvasVoiceWave";
import { SvgVoiceWave } from "./SvgVoiceWave";
import { TranscriptResultCard } from "./TranscriptResultCard";
import { VoiceSetupCards, type VoiceSetupTarget } from "./VoiceSetupCards";

export function VoiceStageLayout({
  runtime,
  onOpenSettings,
}: {
  runtime: DictationRuntime;
  onOpenSettings: (target: VoiceSetupTarget) => void;
}) {
  const { snapshot, readiness, settingsSummary, start, stop, toggleWakeWord } =
    useDictationDashboard(runtime);
  const [remaining, setRemaining] = useState(2.6);
  const [debugVoiceLevel, setDebugVoiceLevel] = useState(18);
  const [waveRenderer, setWaveRenderer] = useState<"svg" | "canvas">("svg");

  useEffect(() => {
    if (snapshot?.phase !== "listening") {
      setRemaining(2.6);
      return undefined;
    }

    const timer = window.setInterval(() => {
      setRemaining((value) =>
        value <= 0.1 ? 2.6 : Number((value - 0.1).toFixed(1)),
      );
    }, 100);

    return () => window.clearInterval(timer);
  }, [snapshot?.phase]);

  if (!snapshot) return null;

  const isListening = snapshot.phase === "listening";
  const isProcessing = ["transcribing", "processing", "injecting"].includes(
    snapshot.phase,
  );
  const status = phaseCopy[snapshot.phase];
  const displayStatus = isProcessing
    ? {
        label: "Обрабатываю…",
        hint: "Fono завершает диктовку и вставляет текст.",
      }
    : status;
  const actionLabel = isListening
    ? "Остановить диктовку"
    : isProcessing
      ? "Обрабатываю…"
      : "Начать диктовку";
  const waveIntensity = debugVoiceLevel / 100;
  const showDebug = window.location.hostname === "localhost";

  return (
    <div className="v2-main-content__inner">
      <section className="v2-voice-stage-layout">
        <VoiceSetupCards
          readiness={readiness}
          settingsSummary={settingsSummary}
          onConfigure={onOpenSettings}
          onToggleWakeWord={toggleWakeWord}
        />
        <div className="v2-voice-wave-slot">
          {waveRenderer === "svg" ? (
            <SvgVoiceWave intensity={waveIntensity} active={isListening} />
          ) : (
            <CanvasVoiceWave intensity={waveIntensity} active={isListening} />
          )}
        </div>
        <div className="v2-voice-stage-layout__action">
          <button
            className={`v2-glow-outline-button v2-voice-stage-layout__primary-action ${
              isProcessing ? "is-processing" : ""
            }`}
            type="button"
            onClick={isListening ? stop : start}
            disabled={!isListening && snapshot.phase !== "idle"}
          >
            <MicIcon />
            {actionLabel}
            <span
              className={`v2-voice-stage-layout__pause-indicator ${
                isListening ? "is-active" : ""
              }`}
              aria-hidden="true"
            >
              <i
                style={
                  {
                    "--progress": `${isListening ? (remaining / 2.6) * 100 : 0}%`,
                  } as CSSProperties
                }
              />
            </span>
          </button>
          <p className="v2-voice-stage-layout__hint">{displayStatus.hint}</p>
          <span className="v2-voice-stage-layout__hotkey">
            <kbd>{snapshot.hotkey}</kbd>
            {isListening ? " закончить запись" : " начать запись"}
          </span>
        </div>
        <div className="v2-voice-stage-layout__spacer" aria-hidden="true" />
        <TranscriptResultCard
          transcript={snapshot.transcript}
          isProcessing={isProcessing}
        />
        {showDebug && (
          <aside className="v2-voice-debug-panel">
            <span>DEBUG · голос</span>
            <strong>{debugVoiceLevel}%</strong>
            <input
              type="range"
              min="0"
              max="100"
              value={debugVoiceLevel}
              onChange={(event) =>
                setDebugVoiceLevel(Number(event.target.value))
              }
              aria-label="Имитировать уровень голоса"
            />
            <small>Меняет амплитуду волны</small>
            <div
              className="v2-voice-debug-panel__renderer"
              role="group"
              aria-label="Wave renderer"
            >
              <button
                className={waveRenderer === "svg" ? "is-active" : ""}
                type="button"
                onClick={() => setWaveRenderer("svg")}
              >
                SVG
              </button>
              <button
                className={waveRenderer === "canvas" ? "is-active" : ""}
                type="button"
                onClick={() => setWaveRenderer("canvas")}
              >
                Canvas
              </button>
            </div>
          </aside>
        )}
      </section>
    </div>
  );
}
