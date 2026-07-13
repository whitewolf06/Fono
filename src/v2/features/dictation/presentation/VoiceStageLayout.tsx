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
    "M0 111 C48 111 63 110 88 104 C113 98 121 75 146 75 C171 75 183 132 213 132 C245 132 255 98 286 98 C316 98 332 118 360 118 C394 118 410 102 440 102 C471 102 486 120 519 120 C553 120 564 91 593 91 C623 91 636 135 668 135 C702 135 713 69 743 69 C774 69 783 110 814 110 C843 110 858 111 900 111";
  const spikeClusters = [
    [86, 34, 186],
    [104, 61, 159],
    [121, 8, 212],
    [140, 43, 177],
    [159, 72, 148],
    [177, 93, 127],
    [702, 94, 126],
    [720, 64, 156],
    [739, 10, 210],
    [758, 40, 180],
    [777, 69, 151],
    [795, 91, 129],
  ];
  const ambientSpikes = [
    [38, 95, 127],
    [57, 85, 137],
    [246, 99, 123],
    [267, 88, 134],
    [286, 94, 128],
    [595, 97, 125],
    [615, 87, 135],
    [635, 99, 123],
    [839, 87, 135],
    [860, 98, 124],
  ];
  const spikeScale = active ? 0.9 + intensity * 1.2 : 0.72 + intensity * 0.55;
  const scaleSpikePoint = (value: number) => 110 + (value - 110) * spikeScale;

  return (
    <svg
      className={`v2-voice-wave ${active ? "is-active" : ""}`}
      style={
        {
          "--idle-wave-scale": 1.02 + intensity * 0.42,
          "--active-wave-scale": 1.1 + intensity * 1.25,
        } as CSSProperties
      }
      viewBox="0 0 900 220"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <defs>
        <linearGradient
          id="v2-voice-wave-gradient"
          x1="0"
          y1="0"
          x2="900"
          y2="0"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0" stopColor="#1478c8" stopOpacity="0" />
          <stop offset=".16" stopColor="#35baff" />
          <stop offset=".5" stopColor="#d6fbff" />
          <stop offset=".84" stopColor="#35baff" />
          <stop offset="1" stopColor="#1478c8" stopOpacity="0" />
        </linearGradient>
      </defs>
      <g className="v2-voice-wave__spikes v2-voice-wave__spikes--ambient">
        {ambientSpikes.map(([x, y1, y2]) => (
          <line
            key={x}
            x1={x}
            y1={scaleSpikePoint(y1)}
            x2={x}
            y2={scaleSpikePoint(y2)}
          />
        ))}
      </g>
      <g className="v2-voice-wave__spikes v2-voice-wave__spikes--cluster">
        {spikeClusters.map(([x, y1, y2]) => (
          <line
            key={x}
            x1={x}
            y1={scaleSpikePoint(y1)}
            x2={x}
            y2={scaleSpikePoint(y2)}
          />
        ))}
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
