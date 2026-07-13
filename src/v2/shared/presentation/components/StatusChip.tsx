import type { ReactNode } from "react";
import { StatusDot } from "./StatusDot";

interface StatusChipProps {
  tone?: "ready" | "active" | "muted";
  showWaveform?: boolean;
  children: ReactNode;
}

export function StatusChip({
  tone = "ready",
  showWaveform = false,
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
    </span>
  );
}
