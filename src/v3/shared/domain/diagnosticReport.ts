export interface DiagnosticReport {
  schemaVersion: 1;
  runtime: "native" | "mock";
  text: string;
}

export interface DiagnosticReportPort {
  collect(): Promise<DiagnosticReport>;
}
