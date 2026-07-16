import { useMemo, useState } from "react";
import { previewCommand, type LaunchAppDraft } from "../domain/commandCatalog";

interface CommandsDraft {
  hotkey: string;
  volumeStep: number;
  applications: LaunchAppDraft[];
  testPhrase: string;
}

const initialDraft: CommandsDraft = {
  hotkey: "Ctrl+Shift+Space",
  volumeStep: 10,
  applications: [],
  testPhrase: "",
};

export function useCommandsDraft() {
  const [draft, setDraft] = useState(initialDraft);

  const preview = useMemo(
    () =>
      previewCommand(draft.testPhrase, draft.volumeStep, draft.applications),
    [draft.applications, draft.testPhrase, draft.volumeStep],
  );

  const update = <Key extends keyof CommandsDraft>(
    key: Key,
    value: CommandsDraft[Key],
  ) => {
    setDraft((current) => ({ ...current, [key]: value }));
  };

  const addApplication = () => {
    setDraft((current) => ({
      ...current,
      applications: [
        ...current.applications,
        {
          id: `application-${current.applications.length + 1}`,
          name: "",
          executablePath: "",
          aliases: "",
        },
      ],
    }));
  };

  const updateApplication = (
    id: string,
    key: keyof Omit<LaunchAppDraft, "id">,
    value: string,
  ) => {
    setDraft((current) => ({
      ...current,
      applications: current.applications.map((application) =>
        application.id === id ? { ...application, [key]: value } : application,
      ),
    }));
  };

  const removeApplication = (id: string) => {
    setDraft((current) => ({
      ...current,
      applications: current.applications.filter(
        (application) => application.id !== id,
      ),
    }));
  };

  return {
    addApplication,
    draft,
    preview,
    removeApplication,
    update,
    updateApplication,
  };
}
