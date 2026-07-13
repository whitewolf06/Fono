import { useEffect, useState, type CSSProperties } from "react";
import { phaseCopy } from "@/v2/shared/domain/pipeline";
import { StatusChip } from "@/v2/shared/presentation/components/StatusChip";
import { useDictationDashboard } from "../application/useDictationDashboard";
import type { DictationRuntime } from "../application/dictationRuntime";

export function VoiceStageLayout({ runtime }: { runtime: DictationRuntime }) {
  const { snapshot, start, stop } = useDictationDashboard(runtime);
  const [remaining, setRemaining] = useState(2.6);
  const [debugVoiceLevel, setDebugVoiceLevel] = useState(18);

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
  const status = phaseCopy[snapshot.phase];
  const waveIntensity = debugVoiceLevel / 100;
  const showDebug = window.location.hostname === "localhost";

  return (
    <section className="v2-voice-stage-layout">
      <button
        className="v2-voice-status"
        type="button"
        onClick={isListening ? stop : start}
        disabled={!isListening && snapshot.phase !== "idle"}
      >
        <StatusChip tone={isListening ? "active" : "ready"} showWaveform>
          {status.label}
        </StatusChip>
      </button>
      <VoiceWave intensity={waveIntensity} active={isListening} />
      <div
        className="v2-voice-workspace__spirit"
        role="img"
        aria-label="Огонёк Fono"
      />
      {isListening && (
        <div className="v2-voice-live-panel">
          <div>
            <span>Пауза до перевода</span>
            <b>{remaining.toFixed(1)} с</b>
          </div>
          <i
            style={
              {
                "--countdown": `${(remaining / 2.6) * 100}%`,
              } as CSSProperties
            }
          />
          <p>{snapshot.transcript || "Слушаю вас…"}</p>
        </div>
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
            onChange={(event) => setDebugVoiceLevel(Number(event.target.value))}
            aria-label="Имитировать уровень голоса"
          />
          <small>Меняет амплитуду волны</small>
        </aside>
      )}
    </section>
  );
}

function VoiceWave({
  intensity,
  active,
}: {
  intensity: number;
  active: boolean;
}) {
  const path =
    "M0 110 C60 110 64 106 92 106 C124 106 133 78 164 78 C196 78 205 123 241 123 C278 123 287 91 320 91 C352 91 366 113 397 113 C426 113 438 102 462 102 C488 102 501 116 532 116 C564 116 579 89 611 89 C646 89 655 124 692 124 C731 124 741 79 772 79 C805 79 813 109 900 109";

  return (
    <svg
      className={`v2-voice-wave ${active ? "is-active" : ""}`}
      style={
        {
          "--idle-wave-scale": 1 + intensity * 0.24,
          "--idle-spike-scale": 0.66 + intensity * 0.32,
          "--active-wave-scale": 1 + intensity * 0.9,
          "--active-spike-scale": 0.45 + intensity,
        } as CSSProperties
      }
      viewBox="0 0 900 220"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <linearGradient id="v2-voice-wave-gradient" x1="0" x2="1">
          <stop offset="0" stopColor="#1478c8" stopOpacity="0" />
          <stop offset=".16" stopColor="#35baff" />
          <stop offset=".5" stopColor="#d6fbff" />
          <stop offset=".84" stopColor="#35baff" />
          <stop offset="1" stopColor="#1478c8" stopOpacity="0" />
        </linearGradient>
      </defs>
      <g className="v2-voice-wave__spikes">
        <line x1="102" y1="42" x2="102" y2="178" />
        <line x1="118" y1="71" x2="118" y2="149" />
        <line x1="136" y1="24" x2="136" y2="196" />
        <line x1="158" y1="62" x2="158" y2="158" />
        <line x1="276" y1="40" x2="276" y2="180" />
        <line x1="301" y1="78" x2="301" y2="142" />
        <line x1="598" y1="76" x2="598" y2="144" />
        <line x1="625" y1="37" x2="625" y2="183" />
        <line x1="744" y1="57" x2="744" y2="163" />
        <line x1="768" y1="22" x2="768" y2="198" />
      </g>
      <path className="v2-voice-wave__echo" d={path} />
      <path className="v2-voice-wave__line" d={path} />
      <g className="v2-voice-wave__nodes">
        <circle cx="92" cy="106" r="3" />
        <circle cx="241" cy="123" r="3" />
        <circle cx="397" cy="113" r="2.5" />
        <circle cx="532" cy="116" r="3" />
        <circle cx="692" cy="124" r="3" />
        <circle cx="772" cy="79" r="3" />
      </g>
    </svg>
  );
}
