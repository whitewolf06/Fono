import type { CommandsPort } from "../../../shared/domain/contracts";
import type { NativeContext } from "../../../shared/infrastructure/native/context";
import { call } from "../../../shared/infrastructure/native/ipc";
import { supportedCommands } from "../domain/catalog";
export function nativeCommands(ctx: NativeContext): CommandsPort {
  const { state } = ctx;
  return {
    async confirm() {
      await call("confirm_voice_command");
      state.commandProposal = "";
    },
    async dismiss() {
      await call("cancel_voice_command");
      state.commandProposal = "";
    },
    async saveApp(app) {
      if (!app.name.trim() || !app.path.trim() || !app.phrase.trim())
        throw new Error("Заполните все поля приложения");
      await ctx.saveRaw((settings) => {
        const value = {
          name: app.name,
          exe_path: app.path,
          aliases: app.phrase
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean),
        };
        if (app.id) settings.launch_apps[Number(app.id)] = value;
        else settings.launch_apps.push(value);
        return settings;
      });
    },
    async removeApp(id) {
      await ctx.saveRaw((settings) => {
        settings.launch_apps.splice(Number(id), 1);
        return settings;
      });
    },
    test(phrase) {
      const value = phrase.trim().toLocaleLowerCase("ru");
      const windowPrefix = [
        "переключись на ",
        "перейди в ",
        "перейди на ",
        "открой ",
        "включи ",
      ].find((p) => value.startsWith(p) && value.length > p.length);
      if (windowPrefix)
        return (
          "Переключиться на открытое окно «" +
          value.slice(windowPrefix.length) +
          "». Проверка не выполняет действие."
        );
      const command = supportedCommands.find((c) => c.phrase === value);
      const app = state.applications.find((a) =>
        a.phrase
          .split(",")
          .some(
            (p) =>
              value === p.trim().toLowerCase() ||
              ["запусти ", "старт ", "открыть "].some(
                (prefix) => value === prefix + p.trim().toLowerCase(),
              ),
          ),
      );
      return command
        ? "Команда: " +
            command.description +
            ". Проверка не выполняет действие."
        : app
          ? "Запустить «" + app.name + "». Проверка не запускает приложение."
          : "Фраза не найдена в каталоге.";
    },
  };
}
