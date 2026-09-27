import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useSettingsDraft } from "../application/useSettingsDraft";
import type { SettingsDraftStore } from "../application/useSettingsDraft";
import { RangeField, SettingsStatus } from "./SettingsPrimitives";
import { WakePhraseInput } from "./WakePhraseInput";
import { Switch } from "@/v2/shared/presentation/components/Switch";

export type QuickSettingsTarget =
  "microphone" | "wake-word" | "recognition" | "post-processing";

const dialogCopy: Record<
  QuickSettingsTarget,
  { title: string; description: string }
> = {
  microphone: {
    title: "Микрофон",
    description: "Выберите устройство и проверьте запись.",
  },
  "wake-word": {
    title: "Wake word",
    description: "Настройте ключевую фразу и чувствительность активации.",
  },
  recognition: {
    title: "Распознавание",
    description: "Выберите Whisper-модель и способ ускорения.",
  },
  "post-processing": {
    title: "Постобработка",
    description:
      "Настройте очистку и форматирование текста после распознавания.",
  },
};

export function QuickSettingsDialog({
  target,
  onClose,
  store,
}: {
  target: QuickSettingsTarget;
  onClose: () => void;
  store?: SettingsDraftStore;
}) {
  const {
    draft,
    effectiveAcceleration,
    hasMicrophoneSample,
    lmStudioStatus,
    microphoneStatus,
    playMicrophoneSample,
    reloadWhisperModel,
    saveSettings,
    saveState,
    testLmStudio,
    testMicrophone,
    testWakeWord,
    update,
    wakeWordStatus,
    whisperStatus,
  } = useSettingsDraft(store);
  const copy = dialogCopy[target];
  const wakeWordDisabled = !draft.wakeWordEnabled;
  const processingDisabled = !draft.processingEnabled;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  useEffect(() => {
    if (saveState !== "saved") return undefined;

    const timer = window.setTimeout(onClose, 650);
    return () => window.clearTimeout(timer);
  }, [onClose, saveState]);

  return createPortal(
    <div
      className="v2-quick-settings-backdrop"
      role="presentation"
      onMouseDown={onClose}
    >
      <section
        className="v2-quick-settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="quick-settings-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header>
          <div>
            <span>Настройка</span>
            <h2 id="quick-settings-title">{copy.title}</h2>
            <p>{copy.description}</p>
          </div>
          <button
            className="v2-quick-settings-dialog__close"
            type="button"
            aria-label="Закрыть"
            onClick={onClose}
          >
            ×
          </button>
        </header>
        <div className="v2-quick-settings-dialog__body">
          {target === "microphone" && (
            <>
              <DialogField label="Устройство ввода">
                <select
                  aria-label="Микрофон"
                  value={draft.microphone}
                  onChange={(event) => update("microphone", event.target.value)}
                >
                  {draft.microphoneOptions.map((microphone) => (
                    <option key={microphone}>{microphone}</option>
                  ))}
                </select>
              </DialogField>
              <div className="v2-quick-settings-dialog__actions">
                <button
                  className="v2-button v2-button--primary"
                  type="button"
                  onClick={testMicrophone}
                >
                  Записать тест
                </button>
                <button
                  className="v2-button"
                  type="button"
                  disabled={!hasMicrophoneSample}
                  onClick={playMicrophoneSample}
                >
                  Прослушать
                </button>
              </div>
              <SettingsStatus status={microphoneStatus} />
            </>
          )}

          {target === "wake-word" && (
            <>
              <DialogField label="Включить wake word" inline>
                <Switch
                  checked={draft.wakeWordEnabled}
                  onChange={(value) => update("wakeWordEnabled", value)}
                />
              </DialogField>
              <DialogField label="Ключевая фраза" disabled={wakeWordDisabled}>
                <WakePhraseInput
                  disabled={wakeWordDisabled}
                  draft={draft}
                  onChange={(value) => update("wakePhrase", value)}
                />
              </DialogField>
              <RangeField
                disabled={wakeWordDisabled}
                label="Чувствительность"
                value={draft.wakeSensitivity}
                suffix="%"
                onChange={(value) => update("wakeSensitivity", value)}
              />
              <div className="v2-quick-settings-dialog__actions">
                <button
                  className="v2-button"
                  type="button"
                  disabled={wakeWordDisabled}
                  onClick={testWakeWord}
                >
                  Проверить фразу
                </button>
              </div>
              <SettingsStatus status={wakeWordStatus} />
            </>
          )}

          {target === "recognition" && (
            <>
              <DialogField label="Whisper-модель">
                <div className="v2-quick-settings-dialog__control-stack">
                  <select
                    value={draft.recognitionModel}
                    onChange={(event) =>
                      update("recognitionModel", event.target.value)
                    }
                  >
                    {draft.recognitionModelOptions.map((model) => (
                      <option key={model}>{model}</option>
                    ))}
                  </select>
                  <button
                    className="v2-button"
                    type="button"
                    onClick={reloadWhisperModel}
                  >
                    Загрузить
                  </button>
                </div>
              </DialogField>
              <DialogField label="Ускорение">
                <select
                  value={draft.acceleration}
                  onChange={(event) =>
                    update(
                      "acceleration",
                      event.target.value as typeof draft.acceleration,
                    )
                  }
                >
                  <option value="auto">Авто — рекомендовано</option>
                  <option value="cuda">NVIDIA GPU</option>
                  <option value="vulkan">Другой GPU</option>
                  <option value="cpu">CPU — совместимость</option>
                </select>
              </DialogField>
              <p className="v2-quick-settings-dialog__note">
                Фактически используется:{" "}
                <strong>{effectiveAcceleration}</strong>.
              </p>
              <SettingsStatus status={whisperStatus} />
            </>
          )}

          {target === "post-processing" && (
            <>
              <DialogField
                label="Обрабатывать текст после распознавания"
                inline
              >
                <Switch
                  checked={draft.processingEnabled}
                  onChange={(value) => update("processingEnabled", value)}
                />
              </DialogField>
              <DialogField
                label="Режим обработки"
                disabled={processingDisabled}
              >
                <select
                  disabled={processingDisabled}
                  value={draft.processingMode}
                  onChange={(event) =>
                    update(
                      "processingMode",
                      event.target.value as typeof draft.processingMode,
                    )
                  }
                >
                  <option value="clean">Лёгкая очистка</option>
                  <option value="format">Форматирование</option>
                </select>
              </DialogField>
              <div className="v2-quick-settings-dialog__actions">
                <button
                  className="v2-button"
                  type="button"
                  disabled={processingDisabled}
                  onClick={testLmStudio}
                >
                  Проверить LM Studio
                </button>
              </div>
              <SettingsStatus status={lmStudioStatus} />
            </>
          )}
        </div>
        <footer>
          <button className="v2-button" type="button" onClick={onClose}>
            Отмена
          </button>
          <button
            className="v2-button v2-button--primary"
            type="button"
            disabled={saveState === "saving"}
            onClick={saveSettings}
          >
            {saveState === "saving"
              ? "Сохранение…"
              : saveState === "saved"
                ? "Сохранено"
                : "Сохранить"}
          </button>
        </footer>
      </section>
    </div>,
    document.body,
  );
}

function DialogField({
  children,
  disabled = false,
  inline = false,
  label,
}: {
  children: ReactNode;
  disabled?: boolean;
  inline?: boolean;
  label: string;
}) {
  return (
    <label
      className={`v2-quick-settings-dialog__field ${inline ? "is-inline" : ""} ${disabled ? "is-disabled" : ""}`}
    >
      <span>{label}</span>
      {children}
    </label>
  );
}
