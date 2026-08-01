import { useEffect, useMemo, useState } from "react";
import { previewCommand, type LaunchAppDraft } from "../domain/commandCatalog";

export interface CommandsDraft {
  hotkey: string;
  volumeStep: number;
  applications: LaunchAppDraft[];
  testPhrase: string;
}

export interface CommandsDraftStore {
  load(): Promise<CommandsDraft>;
  save(draft: CommandsDraft): Promise<void>;
}

const initialDraft: CommandsDraft = {
  hotkey: "Ctrl+Shift+Space",
  volumeStep: 10,
  applications: [],
  testPhrase: "",
};

export function useCommandsDraft(store?: CommandsDraftStore) {
  const [draft, setDraft] = useState(initialDraft);
  const [saveState, setSaveState] = useState<
    "idle" | "saving" | "saved" | "error"
  >("idle");

  useEffect(() => {
    if (!store) return;

    let active = true;
    void store.load().then((nextDraft) => {
      if (active) setDraft(nextDraft);
    });

    return () => {
      active = false;
    };
  }, [store]);

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
    setSaveState("idle");
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

  const save = async () => {
    if (!store) return;

    setSaveState("saving");
    try {
      await store.save(draft);
      setSaveState("saved");
    } catch {
      setSaveState("error");
    }
  };

  return {
    addApplication,
    draft,
    preview,
    removeApplication,
    save,
    saveState,
    update,
    updateApplication,
  };
}
