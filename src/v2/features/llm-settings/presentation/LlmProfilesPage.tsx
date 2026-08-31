import { useMemo, useState } from "react";
import type {
  LlmConnectionKind,
  LlmProfile,
  LlmProvider,
  Settings,
  SpeechLlmDataScope,
} from "@/lib/types";
import type { LlmProfilesStore } from "../application/useLlmProfiles";
import { useLlmProfiles } from "../application/useLlmProfiles";

export function LlmProfilesPage({ store }: { store: LlmProfilesStore }) {
  const manager = useLlmProfiles(store);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = useMemo(
    () =>
      manager.settings?.llm_profiles.find(
        (profile) => profile.id === selectedId,
      ) ?? manager.settings?.llm_profiles[0],
    [manager.settings, selectedId],
  );
  if (manager.isLoading || !manager.settings)
    return (
      <section className="v2-llm-page">
        <p>Загружаем LLM-профили…</p>
      </section>
    );
  const settings = manager.settings;
  return (
    <section className="v2-llm-page">
      <header className="v2-page-heading">
        <div>
          <p className="v2-kicker">AI</p>
          <h1>LLM-профили</h1>
          <p>
            Подключения хранятся отдельно от речевого тренера. Ключи остаются в
            Windows Credential Manager.
          </p>
        </div>
        <button
          type="button"
          onClick={() => selected && void manager.test(selected.id)}
        >
          Проверить выбранный
        </button>
      </header>
      {manager.error && <p className="v2-llm-page__error">{manager.error}</p>}
      {manager.message && (
        <p className="v2-llm-page__message">{manager.message}</p>
      )}
      <div className="v2-llm-layout">
        <aside className="v2-llm-profiles" aria-label="Профили LLM">
          {settings.llm_profiles.map((profile) => (
            <button
              key={profile.id}
              type="button"
              className={profile.id === selected?.id ? "is-active" : ""}
              onClick={() => setSelectedId(profile.id)}
            >
              <strong>{profile.name}</strong>
              <span>
                {profile.connection === "cloud" ? "Облако" : "Локально"} ·{" "}
                {profile.has_api_key ? "ключ сохранён" : "без ключа"}
              </span>
            </button>
          ))}
          <button
            type="button"
            className="v2-llm-add"
            onClick={() => void manager.save(addProfile(settings))}
          >
            + Добавить профиль
          </button>
        </aside>
        {selected && (
          <ProfileEditor
            key={selected.id}
            settings={settings}
            profile={selected}
            isSaving={manager.isSaving}
            onSave={manager.save}
          />
        )}
      </div>
    </section>
  );
}

function ProfileEditor({
  settings,
  profile,
  isSaving,
  onSave,
}: {
  settings: Settings;
  profile: LlmProfile;
  isSaving: boolean;
  onSave: (next: Settings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(profile);
  const [apiKey, setApiKey] = useState("");
  const [correctionEnabled, setCorrectionEnabled] = useState(
    settings.text_correction_llm.profile_id === profile.id,
  );
  const [correctionModel, setCorrectionModel] = useState(
    settings.text_correction_llm.model ?? "",
  );
  const [analysisEnabled, setAnalysisEnabled] = useState(
    settings.speech_analysis_llm.enabled &&
      settings.speech_analysis_llm.profile_id === profile.id,
  );
  const [analysisModel, setAnalysisModel] = useState(
    settings.speech_analysis_llm.model ?? "",
  );
  const [scope, setScope] = useState<SpeechLlmDataScope>(
    settings.speech_analysis_llm.data_scope,
  );
  const [cloudConsent, setCloudConsent] = useState(
    settings.speech_analysis_llm.cloud_consent,
  );
  const update = <Key extends keyof LlmProfile>(
    key: Key,
    value: LlmProfile[Key],
  ) => setDraft((current) => ({ ...current, [key]: value }));
  const save = async () => {
    const profiles = settings.llm_profiles.map((candidate) =>
      candidate.id === profile.id
        ? {
            ...draft,
            api_key: apiKey || null,
            has_api_key: apiKey ? true : draft.has_api_key,
          }
        : candidate,
    );
    const next: Settings = {
      ...settings,
      llm_profiles: profiles,
      text_correction_llm: correctionEnabled
        ? {
            profile_id: draft.id,
            model: correctionModel || null,
          }
        : settings.text_correction_llm.profile_id === profile.id
          ? { profile_id: null, model: null }
          : settings.text_correction_llm,
      speech_analysis_llm: analysisEnabled
        ? {
            ...settings.speech_analysis_llm,
            enabled: true,
            profile_id: draft.id,
            model: analysisModel || null,
            data_scope: scope,
            cloud_consent: cloudConsent,
          }
        : settings.speech_analysis_llm.profile_id === profile.id
          ? { ...settings.speech_analysis_llm, enabled: false }
          : settings.speech_analysis_llm,
    };
    await onSave(next);
    setApiKey("");
  };
  return (
    <form
      className="v2-llm-editor"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <label>
        <span>Название</span>
        <input
          value={draft.name}
          onChange={(event) => update("name", event.target.value)}
        />
      </label>
      <div className="v2-llm-editor__pair">
        <label>
          <span>Провайдер</span>
          <select
            value={draft.provider}
            onChange={(event) =>
              update("provider", event.target.value as LlmProvider)
            }
          >
            <option value="lmstudio">LM Studio</option>
            <option value="openai">OpenAI</option>
            <option value="custom">OpenAI-compatible</option>
          </select>
        </label>
        <label>
          <span>Размещение</span>
          <select
            value={draft.connection}
            onChange={(event) =>
              update("connection", event.target.value as LlmConnectionKind)
            }
          >
            <option value="local">Локально</option>
            <option value="cloud">Облако</option>
          </select>
        </label>
      </div>
      <label>
        <span>URL API</span>
        <input
          value={draft.base_url}
          onChange={(event) => update("base_url", event.target.value)}
        />
      </label>
      <label>
        <span>Модель</span>
        <input
          value={draft.model ?? ""}
          placeholder="например, qwen3"
          onChange={(event) => update("model", event.target.value || null)}
        />
      </label>
      <label>
        <span>Новый API-ключ</span>
        <input
          type="password"
          value={apiKey}
          placeholder={
            draft.has_api_key
              ? "Ключ уже сохранён"
              : "Необязательно для локального сервера"
          }
          onChange={(event) => setApiKey(event.target.value)}
        />
      </label>
      <fieldset>
        <legend>Потребители</legend>
        <label className="v2-llm-checkbox">
          <input
            type="checkbox"
            checked={correctionEnabled}
            onChange={(event) => setCorrectionEnabled(event.target.checked)}
          />
          Корректор текста
        </label>
        {correctionEnabled && (
          <label>
            <span>Модель для корректора</span>
            <input
              value={correctionModel}
              placeholder="По умолчанию из профиля"
              onChange={(event) => setCorrectionModel(event.target.value)}
            />
          </label>
        )}
        <label className="v2-llm-checkbox">
          <input
            type="checkbox"
            checked={analysisEnabled}
            onChange={(event) => setAnalysisEnabled(event.target.checked)}
          />
          Речевой анализатор
        </label>
        {analysisEnabled && (
          <label>
            <span>Что отправлять анализатору</span>
            <select
              value={scope}
              onChange={(event) =>
                setScope(event.target.value as SpeechLlmDataScope)
              }
            >
              <option value="metrics_only">Только метрики</option>
              <option value="findings">Метрики и локальные находки</option>
              <option value="original_text">Полный исходный текст</option>
            </select>
          </label>
        )}
        {analysisEnabled && (
          <label>
            <span>Модель для анализатора</span>
            <input
              value={analysisModel}
              placeholder="По умолчанию из профиля"
              onChange={(event) => setAnalysisModel(event.target.value)}
            />
          </label>
        )}
        {analysisEnabled &&
          draft.connection === "cloud" &&
          scope !== "metrics_only" && (
            <label className="v2-llm-checkbox">
              <input
                type="checkbox"
                checked={cloudConsent}
                onChange={(event) => setCloudConsent(event.target.checked)}
              />
              Разрешаю отправлять выбранные данные в облачный LLM
            </label>
          )}
      </fieldset>
      <button type="submit" disabled={isSaving}>
        Сохранить профиль
      </button>
    </form>
  );
}

function addProfile(settings: Settings): Settings {
  const id = `llm-${crypto.randomUUID().slice(0, 8)}`;
  return {
    ...settings,
    llm_profiles: [
      ...settings.llm_profiles,
      {
        id,
        name: "Новый профиль",
        provider: "custom",
        connection: "local",
        base_url: "http://localhost:1234/v1",
        model: null,
        api_key: null,
        has_api_key: false,
      },
    ],
  };
}
