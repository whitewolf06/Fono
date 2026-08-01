import { ipc } from "@/lib/ipc";
import type { LaunchApp } from "@/lib/types";
import type {
  CommandsDraft,
  CommandsDraftStore,
} from "../application/useCommandsDraft";

export function createTauriCommandsDraftStore(): CommandsDraftStore {
  return {
    load: async () => toDraft(await ipc.getSettings()),
    save: async (draft) => {
      const settings = await ipc.getSettings();
      await ipc.saveSettings({
        ...settings,
        command_hotkey: draft.hotkey.replaceAll(" ", ""),
        volume_step: draft.volumeStep,
        launch_apps: draft.applications.map(toLaunchApp),
      });
    },
  };
}

function toDraft(
  settings: Awaited<ReturnType<typeof ipc.getSettings>>,
): CommandsDraft {
  return {
    hotkey: settings.command_hotkey,
    volumeStep: settings.volume_step,
    applications: settings.launch_apps.map((application, index) => ({
      id: `application-${index}`,
      name: application.name,
      executablePath: application.exe_path,
      aliases: application.aliases.join(", "),
    })),
    testPhrase: "",
  };
}

function toLaunchApp(
  application: CommandsDraft["applications"][number],
): LaunchApp {
  return {
    name: application.name.trim(),
    exe_path: application.executablePath.trim(),
    aliases: application.aliases
      .split(",")
      .map((alias) => alias.trim())
      .filter(Boolean),
  };
}
