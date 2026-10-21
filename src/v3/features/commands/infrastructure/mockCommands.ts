import { message as uiMessage } from "../../../shared/domain/messages";
import type {
  CommandsPort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import { supportedCommands } from "../domain/catalog";
export function createCommandsPort(state: WorkspaceState): CommandsPort {
  return {
    saveApp(app) {
      if (!app.name.trim() || !app.phrase.trim() || !app.path.trim())
        throw new Error(uiMessage("ui.1e54c41be1"));
      if (
        state.applications.some(
          (a) =>
            a.id !== app.id &&
            a.phrase.toLowerCase() === app.phrase.toLowerCase(),
        )
      )
        throw new Error(uiMessage("ui.3da37fb531"));
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
          uiMessage("ui.570f6ef5ce") +
          command.description +
          uiMessage("ui.a76221e108")
        );
      const app = state.applications.find(
        (a) => a.phrase.toLocaleLowerCase("ru") === normalized,
      );
      return app
        ? uiMessage("ui.7dbba6b017") + app.name + uiMessage("ui.43d048f1e3")
        : uiMessage("ui.448f7b9d3f");
    },
  };
}
