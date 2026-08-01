import type { CommandsDraftStore } from "../application/useCommandsDraft";
import { useCommandsDraft } from "../application/useCommandsDraft";
import {
  BuiltInCommandsCard,
  CommandPreviewCard,
  CommandRoadmapCard,
  CommandShortcutCard,
  LaunchApplicationsCard,
} from "./CommandCards";
import { CommandIcon } from "./CommandIcons";
import { PageFrame } from "@/v2/shared/presentation/components/PageFrame";

export function CommandsPage({ store }: { store?: CommandsDraftStore }) {
  const {
    addApplication,
    draft,
    preview,
    removeApplication,
    save,
    saveState,
    update,
    updateApplication,
  } = useCommandsDraft(store);

  const cardProps = {
    addApplication,
    draft,
    removeApplication,
    update,
    updateApplication,
  };

  return (
    <PageFrame
      icon={<CommandIcon />}
      title="Команды"
      description="Управляйте Windows голосом: громкость, медиа, окна и настроенные приложения."
    >
      <div className="v2-commands-page">
        <div className={`v2-commands-savebar is-${saveState}`}>
          <span>
            {saveState === "saved"
              ? "Команды сохранены"
              : saveState === "error"
                ? "Не удалось сохранить команды"
                : "Изменения применятся после сохранения"}
          </span>
          <button
            className="v2-button v2-button--primary"
            type="button"
            disabled={saveState === "saving"}
            onClick={save}
          >
            {saveState === "saving" ? "Сохранение…" : "Сохранить"}
          </button>
        </div>
        <CommandShortcutCard {...cardProps} />
        <BuiltInCommandsCard {...cardProps} />
        <LaunchApplicationsCard {...cardProps} />
        <CommandPreviewCard draft={draft} preview={preview} update={update} />
        <CommandRoadmapCard />
      </div>
    </PageFrame>
  );
}
