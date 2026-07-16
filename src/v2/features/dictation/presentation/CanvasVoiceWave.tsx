import { useEffect, useRef } from "react";

type CanvasVoiceWaveProps = {
  active: boolean;
  intensity: number;
};

const spikePositions = [
  0.1, 0.12, 0.14, 0.16, 0.18, 0.2, 0.72, 0.74, 0.76, 0.78, 0.8, 0.82,
];

const sparks = Array.from({ length: 38 }, (_, index) => ({
  alpha: 0.22 + ((index * 29) % 48) / 100,
  phase: index * 0.73,
  size: 0.65 + ((index * 17) % 10) / 10,
  speed: 0.45 + ((index * 11) % 12) / 10,
  x: 0.12 + ((index * 41) % 76) / 100,
  y: 0.12 + ((index * 23) % 51) / 100,
}));

export function CanvasVoiceWave({ active, intensity }: CanvasVoiceWaveProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;

    if (!canvas) return undefined;

    const context = canvas.getContext("2d");

    if (!context) return undefined;

    let animationFrame = 0;
    let width = 0;
    let height = 0;
    let pixelRatio = 1;

    const resize = () => {
      const bounds = canvas.getBoundingClientRect();

      width = bounds.width;
      height = bounds.height;
      pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(width * pixelRatio);
      canvas.height = Math.round(height * pixelRatio);
      context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    };

    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const draw = (timestamp: number) => {
      const time = timestamp / 1000;

      context.clearRect(0, 0, width, height);
      drawSparks(context, width, height, time, intensity, active);
      drawWave(context, width, height, time, intensity, active);
      drawSpikes(context, width, height, time, intensity, active);

      animationFrame = window.requestAnimationFrame(draw);
    };

    animationFrame = window.requestAnimationFrame(draw);

    return () => {
      window.cancelAnimationFrame(animationFrame);
      observer.disconnect();
    };
  }, [active, intensity]);

  return (
    <canvas ref={canvasRef} className="v2-voice-wave v2-voice-wave--canvas" />
  );
}

function drawWave(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
  intensity: number,
  active: boolean,
) {
  const centerY = height * 0.5;
  const step = 7;
  const baseAmplitude = active ? 12 + intensity * 30 : 5 + intensity * 13;
  const motion = active ? 1.15 + intensity * 1.35 : 0.38 + intensity * 0.62;
  const wavePoint = (x: number) => {
    const progress = x / width;
    const leftCluster = bell(progress, 0.19, 0.07);
    const rightCluster = bell(progress, 0.79, 0.07);
    const clusterEnergy = 1 + (leftCluster + rightCluster) * 1.75;
    const primary = Math.sin(progress * 18 + time * motion);
    const texture = Math.sin(progress * 47 - time * motion * 1.45) * 0.26;

    return centerY + (primary + texture) * baseAmplitude * clusterEnergy;
  };
  const drawPath = () => {
    context.beginPath();
    context.moveTo(0, wavePoint(0));

    for (let x = step; x <= width; x += step) {
      context.lineTo(x, wavePoint(x));
    }
  };

  context.save();
  drawPath();
  context.strokeStyle = "rgba(42, 191, 255, 0.22)";
  context.lineWidth = 5;
  context.shadowBlur = 20;
  context.shadowColor = "rgba(42, 177, 255, 0.78)";
  context.stroke();
  context.restore();

  context.save();
  drawPath();
  context.strokeStyle = "rgba(245, 254, 255, 0.97)";
  context.lineWidth = 1.25;
  context.shadowBlur = 8;
  context.shadowColor = "rgba(92, 209, 255, 0.92)";
  context.stroke();
  context.restore();
}

function drawSpikes(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
  intensity: number,
  active: boolean,
) {
  const centerY = height * 0.5;
  const activity = active ? 0.9 + intensity * 1.1 : 0.72 + intensity * 0.46;
  const pulseRange = active ? 0.12 + intensity * 0.28 : 0.04 + intensity * 0.12;

  spikePositions.forEach((position, index) => {
    const pulse =
      1 + Math.sin(time * (2.2 + (index % 3) * 0.32) + index) * pulseRange;
    const prominence =
      0.48 + bell(position, 0.15, 0.08) + bell(position, 0.77, 0.08);
    const halfLength = (20 + prominence * 76) * activity * pulse;
    const x = width * position;
    const gradient = context.createLinearGradient(
      x,
      centerY - halfLength,
      x,
      centerY + halfLength,
    );

    gradient.addColorStop(0, "rgba(45, 185, 255, 0)");
    gradient.addColorStop(0.34, "rgba(64, 202, 255, 0.72)");
    gradient.addColorStop(0.5, "rgba(239, 254, 255, 0.98)");
    gradient.addColorStop(0.66, "rgba(64, 202, 255, 0.72)");
    gradient.addColorStop(1, "rgba(45, 185, 255, 0)");

    context.save();
    context.strokeStyle = gradient;
    context.lineWidth = index % 3 === 0 ? 1.25 : 0.85;
    context.shadowBlur = 9;
    context.shadowColor = "rgba(52, 184, 255, 0.78)";
    context.beginPath();
    context.moveTo(x, centerY - halfLength);
    context.lineTo(x, centerY + halfLength);
    context.stroke();
    context.restore();
  });
}

function drawSparks(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
  intensity: number,
  active: boolean,
) {
  const activity = active ? 0.5 + intensity * 0.5 : 0.24 + intensity * 0.28;

  context.save();
  sparks.forEach((spark) => {
    const twinkle = 0.4 + Math.sin(time * spark.speed + spark.phase) * 0.34;
    const yOffset = Math.sin(time * spark.speed + spark.phase) * 5 * activity;

    context.globalAlpha = spark.alpha * twinkle * activity;
    context.fillStyle = "#c8f6ff";
    context.shadowBlur = 7;
    context.shadowColor = "#48c8ff";
    context.beginPath();
    context.arc(
      width * spark.x,
      height * spark.y + yOffset,
      spark.size,
      0,
      Math.PI * 2,
    );
    context.fill();
  });
  context.restore();
}

function bell(value: number, center: number, spread: number) {
  return Math.exp(-((value - center) ** 2) / (2 * spread ** 2));
}
