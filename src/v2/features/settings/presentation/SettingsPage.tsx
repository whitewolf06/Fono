import type {
  SettingsDraftStore,
  SettingsSection,
} from "../application/useSettingsDraft";
import { useSettingsDraft } from "../application/useSettingsDraft";
import type { WakeCalibrationStore } from "../application/useWakeCalibration";
import { useWakeCalibration } from "../application/useWakeCalibration";
import type { WakeProfileValidationStore } from "../application/useWakeProfileValidation";
import { useWakeProfileValidation } from "../application/useWakeProfileValidation";
import {
  ActivationSettingsCard,
  AudioSettingsCard,
  GeneralSettingsCard,
} from "./CoreSettingsCards";
import {
  DiagnosticsSettingsCard,
  OverlaySettingsCard,
  PrivacySettingsCard,
  ProcessingSettingsCard,
} from "./ServiceSettingsCards";
import { SectionIcon } from "./SettingsPrimitives";
import { PageFrame } from "@/v2/shared/presentation/components/PageFrame";

interface SettingsPageProps {
  focusSection?: SettingsSection;
  store?: SettingsDraftStore;
  calibrationStore?: WakeCalibrationStore;
  validationStore?: WakeProfileValidationStore;
}

export function SettingsPage({
  focusSection,
  store,
  calibrationStore,
  validationStore,
}: SettingsPageProps) {
  const {
    advancedWakeOpen,
    activateWakeWord,
    collapsedSections,
    draft,
    effectiveAcceleration,
    hasMicrophoneSample,
    lmStudioStatus,
    microphoneStatus,
    overlayStatus,
    playMicrophoneSample,
    processingPreview,
    reloadWhisperModel,
    restoreOriginalTranscript,
    saveSettings,
    saveState,
    setAdvancedWakeOpen,
    showOverlayTest,
    testLmStudio,
    testMicrophone,
    testWakeWord,
    toggleCollapsedSection,
    update,
    wakeWordStatus,
    whisperStatus,
  } = useSettingsDraft(store);
  const calibration = useWakeCalibration(calibrationStore);
  const validation = useWakeProfileValidation(validationStore);

  const sharedProps = { draft, focusSection, update };
  const cardState = (section: SettingsSection) => ({
    collapsed: collapsedSections.includes(section),
    onToggleCollapsed: () => toggleCollapsedSection(section),
  });

  const saveCopy = {
    idle: "Сохранить изменения",
    saving: "Сохранение…",
    saved: "Сохранено",
    error: "Повторить сохранение",
  }[saveState];

  return (
    <PageFrame
      icon={<SectionIcon type="general" />}
      title="Настройки"
      description="Все основные настройки Fono собраны на одной странице."
    >
      <div className="v2-settings-page">
        <div className={`v2-settings-savebar is-${saveState}`}>
          <span>{saveState === "saved" ? "✓" : "•"}</span>
          <p>
            {saveState === "idle"
              ? "Изменения ещё не применены."
              : saveState === "saving"
                ? "Передаю настройки в runtime…"
                : saveState === "saved"
                  ? "Настройки сохранены."
                  : "Не удалось сохранить настройки. Проверьте выбранные устройство и модель."}
          </p>
          <button
            className="v2-button v2-button--primary"
            type="button"
            disabled={saveState === "saving"}
            onClick={saveSettings}
          >
            {saveCopy}
          </button>
        </div>
        <div className="v2-settings-grid">
          <GeneralSettingsCard {...sharedProps} {...cardState("general")} />
          <AudioSettingsCard
            {...sharedProps}
            {...cardState("audio")}
            effectiveAcceleration={effectiveAcceleration}
            hasMicrophoneSample={hasMicrophoneSample}
            microphoneStatus={microphoneStatus}
            whisperStatus={whisperStatus}
            onPlayMicrophoneSample={playMicrophoneSample}
            onReloadWhisperModel={reloadWhisperModel}
            onTestMicrophone={testMicrophone}
          />
          <ActivationSettingsCard
            {...sharedProps}
            {...cardState("activation")}
            advancedWakeOpen={advancedWakeOpen}
            calibration={calibration}
            validation={validation}
            wakeWordStatus={wakeWordStatus}
            onActivateWakeWord={activateWakeWord}
            onTestWakeWord={testWakeWord}
            onToggleAdvancedWake={() => setAdvancedWakeOpen(!advancedWakeOpen)}
          />
          <ProcessingSettingsCard
            {...sharedProps}
            {...cardState("processing")}
            lmStudioStatus={lmStudioStatus}
            processingPreview={processingPreview}
            onRestoreOriginalTranscript={restoreOriginalTranscript}
            onTestLmStudio={testLmStudio}
          />
          <PrivacySettingsCard {...sharedProps} {...cardState("privacy")} />
          <OverlaySettingsCard
            {...sharedProps}
            {...cardState("overlay")}
            overlayStatus={overlayStatus}
            onShowOverlayTest={showOverlayTest}
          />
          <DiagnosticsSettingsCard
            {...sharedProps}
            {...cardState("advanced")}
          />
        </div>
      </div>
    </PageFrame>
  );
}
