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
      const yes = (value: boolean) => (value ? "да" : "нет");
      const jobCount = (status: WorkspaceState["jobs"][number]["state"]) =>
        state.jobs.filter((job) => job.state === status).length;
      const text = [
        "Fono — демонстрационный отчёт диагностики (схема 1)",
        "Runtime: browser mock. Состояние не подтверждает работу микрофона или Rust.",
        `Микрофон доступен в макете: ${yes(state.microphoneAvailable)}`,
        `WakeWord включён в макете: ${yes(prefs.wakeEnabled)}`,
        `Обработка включена в макете: ${yes(prefs.processingEnabled)}`,
        `ИИ доступен в макете: ${yes(state.aiAvailable)}`,
        `История: ${yes(prefs.historyEnabled)} | Тренер: ${yes(prefs.trainerEnabled)}`,
        `API включён в макете: ${yes(prefs.serviceEnabled)}`,
        `Очередь API: ожидают ${jobCount("queued")}; выполняются ${jobCount("running")}; ошибки ${jobCount("error")}`,
        "Режим диктовки: классический. Нативные задержки в браузере не измеряются.",
        "Приватность: отчёт не содержит текстов, аудио, словаря, фразы пробуждения, имён устройств, путей, ключей, адресов подключений, инструкций или журнала.",
        "Отчёт создаётся локально; отправка не выполняется.",
      ].join("\n");
      return { schemaVersion: 1, runtime: "mock", text };
    },
  };
}
