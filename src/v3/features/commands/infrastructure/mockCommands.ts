import type {
  CommandsPort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import { supportedCommands } from "../domain/catalog";
export function createCommandsPort(state: WorkspaceState): CommandsPort {
  return {
    saveApp(app) {
      if (!app.name.trim() || !app.phrase.trim() || !app.path.trim())
        throw new Error("Заполните название, фразу и путь приложения.");
      if (
        state.applications.some(
          (a) =>
            a.id !== app.id &&
            a.phrase.toLowerCase() === app.phrase.toLowerCase(),
        )
      )
        throw new Error("Такая фраза уже занята.");
      const index = state.applications.findIndex((a) => a.id === app.id);
      const next = { ...app, id: app.id || "app-" + Date.now() };
      if (index < 0) state.applications.push(next);
      else state.applications[index] = next;
    },
    removeApp(id) {
      state.applications = state.applications.filter((a) => a.id !== id);
    },
    test(phrase) {
      const normalized = phrase
        .trim()
        .toLocaleLowerCase("ru")
        .replace(/[.!?]/g, "");
      const command = supportedCommands.find((c) => c.phrase === normalized);
      if (command)
        return (
          "Распознано: " +
          command.description +
          ". В демонстрации действие не выполняется."
        );
      const app = state.applications.find(
        (a) => a.phrase.toLocaleLowerCase("ru") === normalized,
      );
      return app
        ? "Распознано: открыть «" +
            app.name +
            "». В демонстрации приложение не запускается."
        : "Команда не найдена. Используйте одну из фраз каталога или произносимое название приложения.";
    },
  };
}
