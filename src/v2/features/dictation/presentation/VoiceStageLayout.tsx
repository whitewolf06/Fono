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
  const motionPhase = useWaveMotion();
  const path = createWavePath({ intensity, active, motionPhase });
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
  const spikeScale = active ? 0.96 + intensity * 0.72 : 0.74 + intensity * 0.46;
  const spikeMotion = active
    ? 0.08 + intensity * 0.28
    : 0.035 + intensity * 0.12;
  const scaleSpikePoint = (value: number, x: number) => {
    const independentPulse = Math.sin(motionPhase * 2.8 + x * 0.085);
    const scale = spikeScale * (1 + independentPulse * spikeMotion);

    return 110 + (value - 110) * scale;
  };

  return (
    <svg
      className={`v2-voice-wave ${active ? "is-active" : ""}`}
      style={
        {
          "--idle-wave-scale": 1.01 + intensity * 0.16,
          "--active-wave-scale": 1.03 + intensity * 0.44,
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
        <linearGradient
          id="v2-voice-spike-gradient"
          x1="0"
          y1="0"
          x2="0"
          y2="220"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0" stopColor="#34bbff" stopOpacity="0" />
          <stop offset="0.34" stopColor="#48c7ff" stopOpacity="0.72" />
          <stop offset="0.5" stopColor="#ecfdff" stopOpacity="1" />
          <stop offset="0.66" stopColor="#48c7ff" stopOpacity="0.72" />
          <stop offset="1" stopColor="#34bbff" stopOpacity="0" />
        </linearGradient>
      </defs>
      <g className="v2-voice-wave__spikes v2-voice-wave__spikes--ambient">
        {ambientSpikes.map(([x, y1, y2]) => (
          <line
            key={x}
            x1={x}
            y1={scaleSpikePoint(y1, x)}
            x2={x}
            y2={scaleSpikePoint(y2, x)}
          />
        ))}
      </g>
      <g className="v2-voice-wave__spikes v2-voice-wave__spikes--cluster">
        {spikeClusters.map(([x, y1, y2]) => (
          <line
            key={x}
            x1={x}
            y1={scaleSpikePoint(y1, x)}
            x2={x}
            y2={scaleSpikePoint(y2, x)}
          />
        ))}
      </g>
      <path className="v2-voice-wave__aura" d={path} />
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

function useWaveMotion() {
  const [motionPhase, setMotionPhase] = useState(0);

  useEffect(() => {
    let animationFrame = 0;
    let previousFrame = 0;

    const animate = (timestamp: number) => {
      if (timestamp - previousFrame >= 40) {
        previousFrame = timestamp;
        setMotionPhase((timestamp / 1000) % (Math.PI * 2));
      }

      animationFrame = window.requestAnimationFrame(animate);
    };

    animationFrame = window.requestAnimationFrame(animate);

    return () => window.cancelAnimationFrame(animationFrame);
  }, []);

  return motionPhase;
}

function createWavePath({
  intensity,
  active,
  motionPhase,
}: {
  intensity: number;
  active: boolean;
  motionPhase: number;
}) {
  const step = 30;
  const baseAmplitude = active ? 18 + intensity * 36 : 9 + intensity * 17;
  const movement = active ? 0.72 + intensity * 0.9 : 0.32 + intensity * 0.5;
  const yAt = (x: number) => {
    const primary = Math.sin(x / 94 + motionPhase * movement);
    const detail = Math.sin(x / 37 - motionPhase * movement * 1.7) * 0.34;
    const drift = Math.sin(x / 174 + motionPhase * movement * 0.56) * 0.26;

    return 110 + (primary + detail + drift) * baseAmplitude;
  };
  let path = `M 0 ${yAt(0).toFixed(2)}`;

  for (let x = step; x <= 900; x += step) {
    const previousX = x - step;
    const previousY = yAt(previousX).toFixed(2);
    const currentY = yAt(x).toFixed(2);

    path += ` C ${previousX + step / 3} ${previousY}, ${x - step / 3} ${currentY}, ${x} ${currentY}`;
  }

  return path;
}
