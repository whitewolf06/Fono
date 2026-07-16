export type StatusTone = "ready" | "active" | "muted" | "processing";

export function StatusDot({ tone = "ready" }: { tone?: StatusTone }) {
  return (
    <span
      className={`v2-status-dot v2-status-dot--${tone}`}
      aria-hidden="true"
    />
  );
}
