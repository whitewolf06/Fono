import type { SettingsDraft } from "../application/useSettingsDraft";
import {
  RangeField,
  SettingRow,
  SettingsCard,
  SignalPreview,
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

interface ActivationSettingsCardProps extends SettingsCardsProps {
  advancedWakeOpen: boolean;
  onToggleAdvancedWake: () => void;
}

export function GeneralSettingsCard({
  draft,
  focusSection,
  update,
}: SettingsCardsProps) {
  return (
    <SettingsCard
      icon="general"
      title="Основное"
      description="Настройки, которые влияют на каждую диктовку."
      focused={focusSection === "general"}
    >
      <SettingRow
        title="Запускать вместе с Windows"
        description="Fono будет готов к диктовке после входа в систему."
      >
        <Switch
          checked={draft.autostart}
          onChange={(checked) => update("autostart", checked)}
        />
      </SettingRow>
      <label className="v2-field">
        <span>Язык распознавания</span>
        <select
          value={draft.language}
          onChange={(event) => update("language", event.target.value)}
        >
          <option value="auto">Автоопределение</option>
          <option value="ru">Русский</option>
          <option value="en">English</option>
        </select>
      </label>
      <label className="v2-field">
        <span>Горячая клавиша диктовки</span>
        <input
          value={draft.hotkey}
          onChange={(event) => update("hotkey", event.target.value)}
        />
      </label>
      <fieldset className="v2-radio-group">
        <legend>Способ вставки текста</legend>
        <label className="v2-radio">
          <input
            type="radio"
            name="insertion-mode"
            checked={draft.insertionMode === "sendinput"}
            onChange={() => update("insertionMode", "sendinput")}
          />
          <span aria-hidden="true" />
          SendInput
        </label>
        <label className="v2-radio">
          <input
            type="radio"
            name="insertion-mode"
            checked={draft.insertionMode === "clipboard"}
            onChange={() => update("insertionMode", "clipboard")}
          />
          <span aria-hidden="true" />
          Буфер обмена
        </label>
      </fieldset>
    </SettingsCard>
  );
}

export function AudioSettingsCard({
  draft,
  focusSection,
  update,
}: SettingsCardsProps) {
  return (
    <SettingsCard
      icon="audio"
      title="Аудио и распознавание"
      description="Источник звука, Whisper-модель и ускоритель обработки."
      focused={focusSection === "audio"}
    >
      <label className="v2-field">
        <span>Микрофон</span>
        <select
          value={draft.microphone}
          onChange={(event) => update("microphone", event.target.value)}
        >
          <option>Microphone Array (Realtek)</option>
          <option>USB Microphone</option>
          <option>Default system device</option>
        </select>
      </label>
      <SignalPreview />
      <label className="v2-field">
        <span>Модель распознавания</span>
        <select
          value={draft.recognitionModel}
          onChange={(event) => update("recognitionModel", event.target.value)}
        >
          <option>Whisper Small</option>
          <option>Whisper Base</option>
          <option>Whisper Medium</option>
        </select>
      </label>
      <label className="v2-field">
        <span>Ускорение</span>
        <select
          value={draft.acceleration}
          onChange={(event) =>
            update(
              "acceleration",
              event.target.value as SettingsDraft["acceleration"],
            )
          }
        >
          <option value="auto">Авто — рекомендуемый backend</option>
          <option value="cuda">CUDA — NVIDIA</option>
          <option value="vulkan">Vulkan — AMD / Intel / NVIDIA</option>
          <option value="cpu">CPU — режим совместимости</option>
        </select>
      </label>
      <p className="v2-settings-note">
        В режиме «Авто» Fono выбирает доступный GPU worker. CPU остаётся
        запасным режимом совместимости.
      </p>
    </SettingsCard>
  );
}

export function ActivationSettingsCard({
  draft,
  focusSection,
  advancedWakeOpen,
  onToggleAdvancedWake,
  update,
}: ActivationSettingsCardProps) {
  return (
    <SettingsCard
      icon="activation"
      title="Активация"
      description="Hotkey для точного управления и wake word для работы без рук."
      focused={focusSection === "activation"}
    >
      <SettingRow
        title="Wake word"
        description="Слушать ключевую фразу в фоне и запускать диктовку."
      >
        <Switch
          checked={draft.wakeWordEnabled}
          onChange={(checked) => update("wakeWordEnabled", checked)}
        />
      </SettingRow>
      <label className="v2-field">
        <span>Ключевая фраза</span>
        <input
          value={draft.wakePhrase}
          disabled={!draft.wakeWordEnabled}
          onChange={(event) => update("wakePhrase", event.target.value)}
        />
      </label>
      <RangeField
        label="Чувствительность"
        value={draft.wakeSensitivity}
        suffix="%"
        onChange={(value) => update("wakeSensitivity", value)}
      />
      <RangeField
        label="Пауза перед распознаванием"
        value={draft.silenceDelay}
        min={1}
        max={6}
        step={0.5}
        suffix=" с"
        onChange={(value) => update("silenceDelay", value)}
      />
      <div className="v2-accordion">
        <button
          className="v2-accordion__trigger"
          type="button"
          aria-expanded={advancedWakeOpen}
          onClick={onToggleAdvancedWake}
        >
          <span>
            <b>Расширенные параметры wake word</b>
            <small>Порог срабатывания и VAD — только для диагностики.</small>
          </span>
          <i>{advancedWakeOpen ? "−" : "+"}</i>
        </button>
        {advancedWakeOpen && (
          <div className="v2-accordion__content">
            <div className="v2-advanced-card">
              <span>Порог срабатывания</span>
              <strong>0.25</strong>
              <small>
                Ниже — чувствительнее, но больше ложных срабатываний.
              </small>
            </div>
            <div className="v2-advanced-card">
              <span>Порог VAD</span>
              <strong>0.015</strong>
              <small>Отделяет речь от фонового шума.</small>
            </div>
          </div>
        )}
      </div>
    </SettingsCard>
  );
}
