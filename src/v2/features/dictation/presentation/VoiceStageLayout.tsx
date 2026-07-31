import { useState } from "react";
import { phaseCopy } from "@/v2/shared/domain/pipeline";
import { MicIcon } from "@/v2/shared/presentation/components/MicIcon";
import { useDictationDashboard } from "../application/useDictationDashboard";
import type { DictationRuntime } from "../application/dictationRuntime";
import { CanvasVoiceWave } from "./CanvasVoiceWave";
import { DictationHistoryPanel } from "./DictationHistoryPanel";
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
  const [debugVoiceLevel, setDebugVoiceLevel] = useState(18);
  const [waveRenderer, setWaveRenderer] = useState<"svg" | "canvas">("svg");
  const [historyOpen, setHistoryOpen] = useState(false);

  if (!snapshot) return null;

  const isListening = snapshot.phase === "listening";
  const isProcessing = ["transcribing", "processing", "injecting"].includes(
    snapshot.phase,
  );
  const status = phaseCopy[snapshot.phase];
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
          <span
            className={`v2-voice-stage-layout__status is-${snapshot.phase}`}
          >
            {status.label}
          </span>
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
          </button>
          <p className="v2-voice-stage-layout__hint">{status.hint}</p>
          <span className="v2-voice-stage-layout__hotkey">
            <kbd>{snapshot.hotkey}</kbd>
            {isListening ? " закончить запись" : " начать запись"}
          </span>
        </div>
        <div className="v2-voice-stage-layout__spacer" aria-hidden="true" />
        <TranscriptResultCard
          transcript={snapshot.transcript}
          isProcessing={isProcessing}
          onOpenHistory={() => setHistoryOpen(true)}
        />
        {historyOpen && (
          <DictationHistoryPanel onClose={() => setHistoryOpen(false)} />
        )}
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
