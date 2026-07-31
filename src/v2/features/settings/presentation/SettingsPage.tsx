import type { SettingsSection } from "../application/useSettingsDraft";
import { useSettingsDraft } from "../application/useSettingsDraft";
import {
  ActivationSettingsCard,
  AudioSettingsCard,
  GeneralSettingsCard,
} from "./CoreSettingsCards";
import {
  DiagnosticsSettingsCard,
  OverlaySettingsCard,
  ProcessingSettingsCard,
} from "./ServiceSettingsCards";
import { SectionIcon } from "./SettingsPrimitives";
import { PageFrame } from "@/v2/shared/presentation/components/PageFrame";

interface SettingsPageProps {
  focusSection?: SettingsSection;
}

export function SettingsPage({ focusSection }: SettingsPageProps) {
  const {
    advancedWakeOpen,
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
  } = useSettingsDraft();

  const sharedProps = { draft, focusSection, update };
  const cardState = (section: SettingsSection) => ({
    collapsed: collapsedSections.includes(section),
    onToggleCollapsed: () => toggleCollapsedSection(section),
  });

  const saveCopy = {
    idle: "Сохранить изменения",
    saving: "Сохранение…",
    saved: "Сохранено",
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
                : "Настройки сохранены."}
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
            wakeWordStatus={wakeWordStatus}
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
