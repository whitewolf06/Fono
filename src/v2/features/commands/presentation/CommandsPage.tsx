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

export function CommandsPage() {
  const {
    addApplication,
    draft,
    preview,
    removeApplication,
    update,
    updateApplication,
  } = useCommandsDraft();

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
        <CommandShortcutCard {...cardProps} />
        <BuiltInCommandsCard {...cardProps} />
        <LaunchApplicationsCard {...cardProps} />
        <CommandPreviewCard draft={draft} preview={preview} update={update} />
        <CommandRoadmapCard />
      </div>
    </PageFrame>
  );
}
