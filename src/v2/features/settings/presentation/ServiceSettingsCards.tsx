import type {
  ProcessingPreview,
  SettingsDraft,
  SettingsStatusDetail,
} from "../application/useSettingsDraft";
import {
  SettingRow,
  SectionIcon,
  SettingsCard,
  SettingsStatus,
} from "./SettingsPrimitives";
import { Switch } from "@/v2/shared/presentation/components/Switch";

export interface SettingsCardBaseProps {
  draft: SettingsDraft;
  focusSection?: string;
  collapsed?: boolean;
  onToggleCollapsed?: () => void;
  update: <Key extends keyof SettingsDraft>(
    key: Key,
    value: SettingsDraft[Key],
  ) => void;
}

interface ProcessingSettingsCardProps extends SettingsCardBaseProps {
  lmStudioStatus: SettingsStatusDetail;
  processingPreview: ProcessingPreview;
  onRestoreOriginalTranscript: () => void;
  onTestLmStudio: () => void;
}

export function ProcessingSettingsCard({
  collapsed,
  draft,
  focusSection,
  lmStudioStatus,
  onRestoreOriginalTranscript,
  onTestLmStudio,
  onToggleCollapsed,
  processingPreview,
  update,
}: ProcessingSettingsCardProps) {
  const processingDisabled = !draft.processingEnabled;
  const displayedText =
    processingPreview.displayedText === "source"
      ? processingPreview.sourceText
      : processingPreview.processedText;

  return (
    <SettingsCard
      collapsed={collapsed}
      icon="processing"
      title="Постобработка"
      description="Необязательная очистка текста перед вставкой."
      focused={focusSection === "processing"}
      onToggleCollapsed={onToggleCollapsed}
    >
      <SettingRow
        title="Обрабатывать текст после распознавания"
        description="Исправлять пунктуацию и очевидные оговорки через выбранный LLM."
      >
        <Switch
          checked={draft.processingEnabled}
          onChange={(checked) => update("processingEnabled", checked)}
        />
      </SettingRow>
      <div
        className={`v2-settings-dependent-group ${processingDisabled ? "is-disabled" : ""}`}
      >
        <SettingRow
          disabled={processingDisabled}
          title="Режим обработки"
          description="Лёгкий режим только очищает текст; форматирование меняет структуру."
        >
          <fieldset
            className="v2-radio-group v2-radio-group--inline"
            disabled={processingDisabled}
          >
            <legend className="v2-sr-only">Режим обработки</legend>
            <label className="v2-radio">
              <input
                type="radio"
                name="processing-mode"
                checked={draft.processingMode === "clean"}
                onChange={() => update("processingMode", "clean")}
              />
              <span aria-hidden="true" />
              Лёгкая очистка
            </label>
            <label className="v2-radio">
              <input
                type="radio"
                name="processing-mode"
                checked={draft.processingMode === "format"}
                onChange={() => update("processingMode", "format")}
              />
              <span aria-hidden="true" />
              Форматирование
            </label>
          </fieldset>
        </SettingRow>
        <article className="v2-settings-service-card">
          <SectionIcon type="processing" />
          <div>
            <span>Провайдер</span>
            <strong>LM Studio · локальный сервер</strong>
            <small>
              Если сервер недоступен или истёк тайм-аут, Fono вставит исходную
              расшифровку.
            </small>
          </div>
          <button
            className="v2-button"
            type="button"
            disabled={processingDisabled}
            onClick={onTestLmStudio}
          >
            Проверить
          </button>
        </article>
        <SettingsStatus status={lmStudioStatus} />
        <div
          className={`v2-settings-text-preview ${processingPreview.fallbackActive ? "is-fallback" : ""}`}
        >
          <div>
            <span>
              {processingPreview.fallbackActive
                ? "Исходный текст"
                : "Результат обработки"}
            </span>
            <p>{displayedText}</p>
          </div>
          <button
            className="v2-button v2-button--ghost"
            type="button"
            disabled={processingPreview.displayedText === "source"}
            onClick={onRestoreOriginalTranscript}
          >
            Вернуть исходный
          </button>
        </div>
      </div>
    </SettingsCard>
  );
}

export function PrivacySettingsCard({
  collapsed,
  draft,
  focusSection,
  onToggleCollapsed,
  update,
}: SettingsCardBaseProps) {
  return (
    <SettingsCard
      collapsed={collapsed}
      icon="advanced"
      title="Данные и приватность"
      description="История остаётся на этом устройстве; аналитика включается только отдельно."
      focused={focusSection === "privacy"}
      onToggleCollapsed={onToggleCollapsed}
    >
      <SettingRow
        title="Хранить историю диктовок"
        description="Сохранять итоговый текст, который Fono вставил в приложение."
      >
        <Switch
          checked={draft.historyEnabled}
          onChange={(checked) => update("historyEnabled", checked)}
        />
      </SettingRow>
      <SettingRow
        title="Разрешить локальную аналитику речи"
        description="Сохранять исходную расшифровку и технические метаданные только в локальной истории. При выключении эти данные удаляются сразу."
      >
        <Switch
          checked={draft.analyticsEnabled}
          onChange={(checked) => update("analyticsEnabled", checked)}
        />
      </SettingRow>
      <SettingRow
        disabled={!draft.analyticsEnabled}
        title="Срок хранения аналитики"
        description="После срока Fono удалит исходный текст и метаданные, оставив итог истории."
      >
        <select
          aria-label="Срок хранения аналитики"
          disabled={!draft.analyticsEnabled}
          value={draft.analyticsRetentionDays}
          onChange={(event) =>
            update("analyticsRetentionDays", Number(event.target.value))
          }
        >
          <option value={7}>7 дней</option>
          <option value={30}>30 дней</option>
          <option value={90}>90 дней</option>
        </select>
      </SettingRow>
    </SettingsCard>
  );
}

export function DiagnosticsSettingsCard({
  collapsed,
  draft,
  onToggleCollapsed,
  update,
}: SettingsCardBaseProps) {
  return (
    <SettingsCard
      collapsed={collapsed}
      icon="advanced"
      title="Диагностика"
      description="Логи и системные ошибки, которые помогут найти проблему."
      onToggleCollapsed={onToggleCollapsed}
    >
      <article className="v2-settings-service-card">
        <SectionIcon type="advanced" />
        <div>
          <strong>Ошибки устройств и ускорения</strong>
          <small>
            Сообщения микрофона, CUDA, Vulkan, wake word и LM Studio появятся
            здесь с понятным действием.
          </small>
        </div>
        <button className="v2-button" type="button">
          Открыть журнал
        </button>
      </article>
      <SettingRow
        title="Подробные логи"
        description="Записывать расширенную информацию для диагностики проблем."
      >
        <Switch
          checked={draft.verboseLogging}
          onChange={(checked) => update("verboseLogging", checked)}
        />
      </SettingRow>
    </SettingsCard>
  );
}
