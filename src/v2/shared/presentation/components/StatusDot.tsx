export function StatusDot({ tone = "ready" }: { tone?: "ready" | "active" | "muted" }) {
  return <span className={`v2-status-dot v2-status-dot--${tone}`} aria-hidden="true" />;
}
