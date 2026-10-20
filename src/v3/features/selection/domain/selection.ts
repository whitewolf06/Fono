export interface SelectionSnapshot {
  operationId: number;
  phase: "reading" | "ready" | "error";
  text?: string;
  source?: "ui_automation" | "clipboard" | "demo";
  message?: string;
}
export interface SelectionCapturePort {
  state: { current: SelectionSnapshot | null };
  cancel(operationId: number): Promise<void>;
  demo?(): void;
  refresh?(): Promise<void>;
  dispose(): void;
}
