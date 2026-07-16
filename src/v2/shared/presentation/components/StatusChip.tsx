import type { CSSProperties, ReactNode } from "react";
import { StatusDot, type StatusTone } from "./StatusDot";

interface StatusChipProps {
  tone?: StatusTone;
  showWaveform?: boolean;
  progress?: number;
  children: ReactNode;
}

export function StatusChip({
  tone = "ready",
  showWaveform = false,
  progress,
  children,
}: StatusChipProps) {
  return (
    <span className={`v2-status-chip v2-status-chip--${tone}`}>
      <StatusDot tone={tone} />
      {children}
      {showWaveform && (
        <span className="v2-status-bars" aria-hidden="true">
          <i />
          <i />
          <i />
          <i />
        </span>
      )}
      {progress !== undefined && (
        <span className="v2-status-chip__progress" aria-hidden="true">
          <i style={{ "--progress": `${progress}%` } as CSSProperties} />
        </span>
      )}
    </span>
  );
}
