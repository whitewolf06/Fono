import { message as uiMessage } from "../../../shared/domain/messages";
import type { WorkspaceState } from "../../../shared/domain/contracts";
import type { DiagnosticReportPort } from "../../../shared/domain/diagnosticReport";

// Copy only numeric and boolean demo state. Never interpolate arbitrary values
// from preferences, errors, device labels, logs, profiles or dictated text.
export function createMockDiagnosticReport(
  state: WorkspaceState,
): DiagnosticReportPort {
  return {
    async collect() {
      const prefs = state.preferences;
      const yes = (value: boolean) =>
        value ? uiMessage("ui.cdc5ed90cb") : uiMessage("ui.dc9337ff5b");
      const jobCount = (status: WorkspaceState["jobs"][number]["state"]) =>
        state.jobs.filter((job) => job.state === status).length;
      const text = [
        uiMessage("ui.b4f41b31ee"),
        uiMessage("ui.4965c32c07"),
        uiMessage("ui.e76504aa61", {
          v0: String(yes(state.microphoneAvailable)),
        }),
        uiMessage("ui.b2c3266cf1", { v0: String(yes(prefs.wakeEnabled)) }),
        uiMessage("ui.6000496257", {
          v0: String(yes(prefs.processingEnabled)),
        }),
        uiMessage("ui.73d9b418c0", { v0: String(yes(state.aiAvailable)) }),
        uiMessage("ui.bff2f61de0", {
          v0: String(yes(prefs.historyEnabled)),
          v1: String(yes(prefs.trainerEnabled)),
        }),
        uiMessage("ui.04c53753ee", { v0: String(yes(prefs.serviceEnabled)) }),
        uiMessage("ui.e3e48796b0", {
          v0: String(jobCount("queued")),
          v1: String(jobCount("running")),
          v2: String(jobCount("error")),
        }),
        uiMessage("ui.85bc8f4fb2"),
        uiMessage("ui.fd3e257e82"),
        uiMessage("ui.0abe0a9c52"),
      ].join("\n");
      return { schemaVersion: 1, runtime: "mock", text };
    },
  };
}
