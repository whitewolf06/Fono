import { ref } from "vue";
import type { DiagnosticReportPort } from "../../../shared/domain/diagnosticReport";

export function useDiagnosticReport(
  report: DiagnosticReportPort,
  copy: (text: string) => Promise<void>,
) {
  const preview = ref("");
  async function createAndCopy() {
    const collected = await report.collect();
    // Keep the generated report in memory so a failed clipboard action still
    // leaves a readable, selectable report. There is no browser persistence.
    preview.value = collected.text;
    await copy(collected.text);
  }
  return { preview, createAndCopy };
}
