export interface SelectionSnapshot {
  operationId: number;
  phase: "reading" | "ready" | "error";
  text?: string;
  source?: "ui_automation" | "clipboard" | "demo";
  message?: string;
  edit?: SelectionEditSnapshot;
}
export interface SelectionEditSnapshot {
  phase:
    | "idle"
    | "starting"
    | "recording"
    | "recognizing"
    | "processing"
    | "cancelling"
    | "ready";
  operationId?: number;
  instruction: string;
  result?: string;
  message?: string;
}
export interface SelectionCapturePort {
  state: { current: SelectionSnapshot | null };
  cancel(operationId: number): Promise<void>;
  startInstruction(operationId: number): Promise<void>;
  stopInstruction(operationId: number, editOperationId: number): Promise<void>;
  cancelEdit(operationId: number, editOperationId: number): Promise<void>;
  transform(
    operationId: number,
    instruction: string,
    transferConfirmed: boolean,
  ): Promise<void>;
  demo?(): void;
  refresh?(): Promise<void>;
  dispose(): void;
}
