import type { Phase } from "../../../shared/domain/contracts";

export function voiceWaveAppearance(phase: Phase, inputLevel: number) {
  const recording = phase === "listening" || phase === "silence";
  const working = phase === "processing" || phase === "transcribing";
  const level = Number.isFinite(inputLevel)
    ? Math.max(0, Math.min(1, inputLevel))
    : 0;
  // Expand quieter speech visually without manufacturing a microphone signal.
  const energy = recording ? Math.sqrt(level) : 0;

  return {
    energy,
    amplitude: recording ? 0.22 + energy * 0.78 : working ? 0.56 : 0.3,
    glow: working ? 6 : 3 + energy * 6,
    opacity: phase === "error" ? 0.55 : 0.74 + energy * 0.26,
    strokeWidth: 1.8 + energy * 1.3,
  };
}
