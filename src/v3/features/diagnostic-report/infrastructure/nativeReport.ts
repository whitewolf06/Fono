import type {
  DiagnosticReport,
  DiagnosticReportPort,
} from "../../../shared/domain/diagnosticReport";
import { call } from "../../../shared/infrastructure/native/ipc";

export function createNativeDiagnosticReport(): DiagnosticReportPort {
  return {
    collect: () => call<DiagnosticReport>("get_diagnostic_report"),
  };
}
