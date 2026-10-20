export interface SelectionSnapshot {
  operationId: number;
  phase: "reading" | "ready" | "error";
  text?: string;
  source?: "ui_automation" | "clipboard" | "demo";
  message?: string;
  edit?: SelectionEditSnapshot;
  delivery?: SelectionDeliverySnapshot;
  autoInsert?: boolean;
  historyEnabled?: boolean;
  historyMessage?: string;
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
  resultId?: number;
  message?: string;
}
export interface SelectionDeliverySnapshot {
  phase:
    | "idle"
    | "armed"
    | "replacing"
    | "copying"
    | "cancelling"
    | "submitted"
    | "blocked"
    | "uncertain"
    | "copied";
  operationId?: number;
  resultId?: number;
  replaceAllowed: boolean;
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
  armReplacement(operationId: number, resultId: number): Promise<void>;
  cancelDelivery(operationId: number, resultId: number): Promise<void>;
  copyResult(operationId: number, resultId: number): Promise<void>;
  insertResult(operationId: number, resultId: number): Promise<void>;
  demoReplace?(): Promise<void>;
  refresh?(): Promise<void>;
  dispose(): void;
}
