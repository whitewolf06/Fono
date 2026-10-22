export type InsertionObservation =
  "not_attempted" | "unknown" | "sent" | "partial" | "focus_changed" | "failed";
export interface ApplicationIdentity {
  id: string;
  name: string;
}
export interface ApplicationContext {
  before: ApplicationIdentity | null;
  after: ApplicationIdentity | null;
  outcome: InsertionObservation;
  method: "sendinput" | "clipboard" | null;
  attempted: boolean;
  demo?: boolean;
}

export function applicationContextFromNative(
  value: unknown,
): ApplicationContext | undefined {
  if (!value || typeof value !== "object") return;
  const v = value as Record<string, unknown>;
  const outcomes = [
    "not_attempted",
    "unknown",
    "sent",
    "partial",
    "focus_changed",
    "failed",
  ];
  if (!outcomes.includes(String(v.outcome)) || typeof v.attempted !== "boolean")
    return;
  const identity = (raw: unknown): ApplicationIdentity | null => {
    if (!raw || typeof raw !== "object") return null;
    const x = raw as Record<string, unknown>;
    if (
      typeof x.name !== "string" ||
      x.name.length > 124 ||
      !/^[\p{L}\p{N} ._-]+$/u.test(x.name) ||
      x.id !== `exe:${x.name.toLowerCase()}.exe`
    )
      return null;
    return { id: String(x.id), name: x.name };
  };
  return {
    before: identity(v.before),
    after: identity(v.after),
    outcome: v.outcome as InsertionObservation,
    attempted: v.attempted,
    method:
      v.method === "sendinput" || v.method === "clipboard" ? v.method : null,
  };
}
