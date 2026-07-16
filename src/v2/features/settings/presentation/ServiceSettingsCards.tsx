import type { SettingsDraft } from "../application/useSettingsDraft";
import {
  RangeField,
  SettingRow,
  SectionIcon,
  SettingsCard,
  Switch,
} from "./SettingsPrimitives";

interface SettingsCardsProps {
  draft: SettingsDraft;
  focusSection?: string;
  update: <Key extends keyof SettingsDraft>(
    key: Key,
    value: SettingsDraft[Key],
  ) => void;
}

export function ProcessingSettingsCard({
  draft,
  focusSection,
  update,
}: SettingsCardsProps) {
  return (
    <SettingsCard
      icon="processing"
      title="Постобработка"
      description="Опциональная очистка текста перед вставкой."
      focused={focusSection === "processing"}
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
      <fieldset className="v2-radio-group">
        <legend>Режим обработки</legend>
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
      <article className="v2-settings-service-card">
        <SectionIcon type="processing" />
        <div>
          <span>Провайдер</span>
          <strong>LM Studio · локальный сервер</strong>
          <small>Подключение будет проверяться при интеграции с runtime.</small>
        </div>
        <button className="v2-button" type="button">
          Настроить
        </button>
      </article>
    </SettingsCard>
  );
}

export function OverlaySettingsCard({
  draft,
  focusSection,
  update,
}: SettingsCardsProps) {
  return (
    <SettingsCard
      icon="overlay"
      title="Overlay"
      description="Отдельное плавающее окно, видимое во время диктовки."
      focused={focusSection === "overlay"}
    >
      <SettingRow
        title="Показывать overlay"
        description="Индикатор состояния появляется поверх других приложений."
      >
        <Switch
          checked={draft.overlayVisible}
          onChange={(checked) => update("overlayVisible", checked)}
        />
      </SettingRow>
      <RangeField
        label="Масштаб"
        value={draft.overlayScale}
        min={80}
        max={130}
        suffix="%"
        onChange={(value) => update("overlayScale", value)}
      />
      <RangeField
        label="Непрозрачность"
        value={draft.overlayOpacity}
        min={55}
        max={100}
        suffix="%"
        onChange={(value) => update("overlayOpacity", value)}
      />
      <SettingRow
        title="Компактный режим"
        description="Показывать только ключевой статус и управление."
      >
        <Switch
          checked={draft.overlayMiniMode}
          onChange={(checked) => update("overlayMiniMode", checked)}
        />
      </SettingRow>
    </SettingsCard>
  );
}

export function DiagnosticsSettingsCard({ draft, update }: SettingsCardsProps) {
  return (
    <SettingsCard
      icon="advanced"
      title="Диагностика"
      description="Тесты и логи вынесены отдельно от ежедневных настроек."
    >
      <article className="v2-settings-service-card">
        <SectionIcon type="audio" />
        <div>
          <strong>Тест микрофона</strong>
          <small>Проверить устройство, уровень сигнала и доступ Windows.</small>
        </div>
        <button className="v2-button" type="button">
          Открыть тест
        </button>
      </article>
      <article className="v2-settings-service-card">
        <SectionIcon type="activation" />
        <div>
          <strong>Отладка wake word</strong>
          <small>Тест WAV, запись образца, модель и последний результат.</small>
        </div>
        <button className="v2-button" type="button">
          Открыть отладку
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
