import type {
  SettingsDraft,
  SettingsStatusDetail,
} from "../application/useSettingsDraft";
import type { WakeCalibrationController } from "../application/useWakeCalibration";
import type { WakeProfileValidationController } from "../application/useWakeProfileValidation";
import {
  RangeField,
  SettingRow,
  SettingsCard,
  SettingsStatus,
  SignalPreview,
} from "./SettingsPrimitives";
import { ShortcutRecorder } from "@/v2/shared/presentation/components/ShortcutRecorder";
import { Switch } from "@/v2/shared/presentation/components/Switch";
import { WakePhraseInput } from "./WakePhraseInput";
import { WakeCalibrationPanel } from "./WakeCalibrationPanel";
import { WakeProfileValidationPanel } from "./WakeProfileValidationPanel";

interface SettingsCardBaseProps {
  draft: SettingsDraft;
  focusSection?: string;
  collapsed?: boolean;
  onToggleCollapsed?: () => void;
  update: <Key extends keyof SettingsDraft>(
    key: Key,
    value: SettingsDraft[Key],
  ) => void;
}

interface AudioSettingsCardProps extends SettingsCardBaseProps {
  effectiveAcceleration: string;
  hasMicrophoneSample: boolean;
  microphoneStatus: SettingsStatusDetail;
  whisperStatus: SettingsStatusDetail;
  onPlayMicrophoneSample: () => void;
  onReloadWhisperModel: () => void;
  onTestMicrophone: () => void;
}

interface ActivationSettingsCardProps extends SettingsCardBaseProps {
  advancedWakeOpen: boolean;
  calibration: WakeCalibrationController;
  validation: WakeProfileValidationController;
  wakeWordStatus: SettingsStatusDetail;
  onActivateWakeWord: () => void;
  onTestWakeWord: () => void;
  onToggleAdvancedWake: () => void;
}

export function GeneralSettingsCard({
  collapsed,
  draft,
  focusSection,
  onToggleCollapsed,
  update,
}: SettingsCardBaseProps) {
  return (
    <SettingsCard
      collapsed={collapsed}
      icon="general"
      title="Основное"
      description="Настройки, которые влияют на каждую диктовку."
      focused={focusSection === "general"}
      onToggleCollapsed={onToggleCollapsed}
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
      <SettingRow
        title="Язык распознавания"
        description="Автоопределение выбирает язык для каждой новой записи."
      >
        <select
          aria-label="Язык распознавания"
          value={draft.language}
          onChange={(event) => update("language", event.target.value)}
        >
          <option value="auto">Автоопределение</option>
          <option value="ru">Русский</option>
          <option value="en">English</option>
        </select>
      </SettingRow>
      <SettingRow
        title="Способ вставки текста"
        description="Прямая вставка быстрее; буфер обмена пригодится в несовместимых приложениях."
      >
        <fieldset className="v2-radio-group v2-radio-group--inline">
          <legend className="v2-sr-only">Способ вставки текста</legend>
          <label className="v2-radio">
            <input
              type="radio"
              name="insertion-mode"
              checked={draft.insertionMode === "sendinput"}
              onChange={() => update("insertionMode", "sendinput")}
            />
            <span aria-hidden="true" />
            <b>Прямая вставка</b>
            <small>SendInput</small>
          </label>
          <label className="v2-radio">
            <input
              type="radio"
              name="insertion-mode"
              checked={draft.insertionMode === "clipboard"}
              onChange={() => update("insertionMode", "clipboard")}
            />
            <span aria-hidden="true" />
            <b>Буфер обмена</b>
          </label>
        </fieldset>
      </SettingRow>
    </SettingsCard>
  );
}

export function AudioSettingsCard({
  collapsed,
  draft,
  effectiveAcceleration,
  focusSection,
  hasMicrophoneSample,
  microphoneStatus,
  onPlayMicrophoneSample,
  onReloadWhisperModel,
  onTestMicrophone,
  onToggleCollapsed,
  update,
  whisperStatus,
}: AudioSettingsCardProps) {
  return (
    <SettingsCard
      collapsed={collapsed}
      icon="audio"
      title="Аудио и распознавание"
      description="Источник звука, Whisper-модель и способ ускорения обработки."
      focused={focusSection === "audio"}
      onToggleCollapsed={onToggleCollapsed}
    >
      <SettingRow
        title="Микрофон"
        description="Выберите устройство, с которого Fono будет получать звук."
      >
        <select
          aria-label="Микрофон"
          value={draft.microphone}
          onChange={(event) => update("microphone", event.target.value)}
        >
          <option>Microphone Array (Realtek)</option>
          <option>USB Microphone</option>
          <option>Default system device</option>
        </select>
      </SettingRow>
      <div className="v2-settings-test-block">
        <SignalPreview />
        <div className="v2-settings-action-row">
          <button
            className="v2-button v2-button--primary"
            type="button"
            onClick={onTestMicrophone}
          >
            Записать тест
          </button>
          <button
            className="v2-button"
            type="button"
            disabled={!hasMicrophoneSample}
            onClick={onPlayMicrophoneSample}
          >
            Прослушать
          </button>
        </div>
        <SettingsStatus status={microphoneStatus} />
      </div>
      <SettingRow
        title="Whisper-модель"
        description="Модель определяет баланс между скоростью и точностью распознавания."
      >
        <div className="v2-settings-control-stack">
          <select
            aria-label="Whisper-модель"
            value={draft.recognitionModel}
            onChange={(event) => update("recognitionModel", event.target.value)}
          >
            {draft.recognitionModelOptions.map((model) => (
              <option key={model}>{model}</option>
            ))}
          </select>
          <button
            className="v2-button"
            type="button"
            onClick={onReloadWhisperModel}
          >
            Загрузить
          </button>
        </div>
      </SettingRow>
      <SettingsStatus status={whisperStatus} />
      <SettingRow
        title="Способ ускорения"
        description="Fono выбирает worker для распознавания; технический backend указан ниже."
      >
        <select
          aria-label="Способ ускорения"
          value={draft.acceleration}
          onChange={(event) =>
            update(
              "acceleration",
              event.target.value as SettingsDraft["acceleration"],
            )
          }
        >
          <option value="auto">Авто — рекомендовано</option>
          <option value="cuda">NVIDIA GPU</option>
          <option value="vulkan">Другой GPU</option>
          <option value="cpu">CPU — совместимость</option>
        </select>
      </SettingRow>
      <p className="v2-settings-runtime-note">
        Фактически используется: <strong>{effectiveAcceleration}</strong>. При
        недоступности GPU Fono переключится на CPU.
      </p>
    </SettingsCard>
  );
}

export function ActivationSettingsCard({
  advancedWakeOpen,
  calibration,
  collapsed,
  draft,
  focusSection,
  onTestWakeWord,
  onActivateWakeWord,
  onToggleAdvancedWake,
  onToggleCollapsed,
  update,
  validation,
  wakeWordStatus,
}: ActivationSettingsCardProps) {
  const wakeWordDisabled = !draft.wakeWordEnabled;
  const calibrationDisabled =
    !draft.wakePhraseIsSupported ||
    draft.wakePhrase.trim().toLocaleLowerCase() !== "рамзи";

  return (
    <SettingsCard
      collapsed={collapsed}
      icon="activation"
      title="Активация"
      description="Горячая клавиша для точного управления и wake word для работы без рук."
      focused={focusSection === "activation"}
      onToggleCollapsed={onToggleCollapsed}
    >
      <ShortcutRecorder
        label="Горячая клавиша диктовки"
        value={draft.hotkey}
        defaultValue="Ctrl + Alt + F"
        conflicts={[{ value: "Ctrl + Shift + Space", label: "режим команд" }]}
        onChange={(value) => update("hotkey", value)}
      />
      <SettingRow
        title="Wake word"
        description="Слушать ключевую фразу в фоне и запускать диктовку."
      >
        <Switch
          checked={draft.wakeWordEnabled}
          onChange={(checked) => update("wakeWordEnabled", checked)}
        />
      </SettingRow>
      <div className="v2-settings-dependent-group">
        <SettingRow
          title="Ключевая фраза"
          description={
            draft.supportsCustomWakePhrase
              ? "Whisper Experimental позволяет ввести произвольную фразу."
              : "Sherpa-ONNX предлагает только проверенные ключевые фразы. Настройте их до включения прослушивания."
          }
        >
          <WakePhraseInput
            aria-label="Ключевая фраза"
            disabled={false}
            draft={draft}
            onChange={(value) => update("wakePhrase", value)}
          />
        </SettingRow>
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
        <div className="v2-settings-action-row">
          <button
            className="v2-button"
            type="button"
            disabled={wakeWordDisabled}
            onClick={onTestWakeWord}
          >
            Проверить фразу
          </button>
        </div>
        <SettingsStatus status={wakeWordStatus} />
        <WakeCalibrationPanel
          calibration={calibration}
          disabled={calibrationDisabled}
        />
        <WakeProfileValidationPanel
          calibration={calibration}
          validation={validation}
          wakeWordEnabled={draft.wakeWordEnabled}
          onActivateWakeWord={onActivateWakeWord}
        />
        <div className="v2-accordion">
          <button
            className="v2-accordion__trigger"
            type="button"
            aria-expanded={advancedWakeOpen}
            onClick={onToggleAdvancedWake}
          >
            <span>
              <b>Дополнительно</b>
              <small>
                Порог срабатывания и VAD — для диагностики wake word.
              </small>
            </span>
            <i>{advancedWakeOpen ? "−" : "+"}</i>
          </button>
          {advancedWakeOpen && (
            <div className="v2-accordion__content">
              <div className="v2-advanced-card">
                <span>Порог срабатывания</span>
                <strong>0.25</strong>
                <small>
                  Ниже — чувствительнее, но выше риск ложных срабатываний.
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
      </div>
    </SettingsCard>
  );
}
