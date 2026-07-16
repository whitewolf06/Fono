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
  const { advancedWakeOpen, draft, setAdvancedWakeOpen, update } =
    useSettingsDraft();
  const sharedProps = { draft, focusSection, update };

  return (
    <PageFrame
      icon={<SectionIcon type="general" />}
      title="Настройки"
      description="Все основные настройки Fono собраны на одной странице."
    >
      <div className="v2-settings-page">
        <div className="v2-settings-grid">
          <GeneralSettingsCard {...sharedProps} />
          <AudioSettingsCard {...sharedProps} />
          <ActivationSettingsCard
            {...sharedProps}
            advancedWakeOpen={advancedWakeOpen}
            onToggleAdvancedWake={() => setAdvancedWakeOpen(!advancedWakeOpen)}
          />
          <ProcessingSettingsCard {...sharedProps} />
          <OverlaySettingsCard {...sharedProps} />
          <DiagnosticsSettingsCard {...sharedProps} />
        </div>
      </div>
    </PageFrame>
  );
}
