import type { SettingsStatusDetail } from "../application/useSettingsDraft";
import type { SettingsCardBaseProps } from "./ServiceSettingsCards";
import {
  RangeField,
  SettingRow,
  SettingsCard,
  SettingsStatus,
} from "./SettingsPrimitives";
import { Switch } from "@/v2/shared/presentation/components/Switch";

interface OverlaySettingsCardProps extends SettingsCardBaseProps {
  overlayStatus: SettingsStatusDetail;
  onShowOverlayTest: () => void;
  onResetOverlayPosition: () => void;
}

export function OverlaySettingsCard({
  collapsed,
  draft,
  focusSection,
  onShowOverlayTest,
  onResetOverlayPosition,
  onToggleCollapsed,
  overlayStatus,
  update,
}: OverlaySettingsCardProps) {
  const overlayDisabled = !draft.overlayVisible;
  const busy = overlayStatus.state === "checking";

  return (
    <SettingsCard
      collapsed={collapsed}
      icon="overlay"
      title="Overlay"
      description="Отдельное плавающее окно, видимое во время диктовки."
      focused={focusSection === "overlay"}
      onToggleCollapsed={onToggleCollapsed}
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
      <div
        className={`v2-settings-dependent-group ${overlayDisabled ? "is-disabled" : ""}`}
      >
        <RangeField
          disabled={overlayDisabled}
          label="Масштаб"
          value={draft.overlayScale}
          min={80}
          max={130}
          suffix="%"
          onChange={(value) => update("overlayScale", value)}
        />
        <RangeField
          disabled={overlayDisabled}
          label="Непрозрачность"
          value={draft.overlayOpacity}
          min={55}
          max={100}
          suffix="%"
          onChange={(value) => update("overlayOpacity", value)}
        />
        <SettingRow
          disabled={overlayDisabled}
          title="Компактный режим"
          description="Показывать только ключевой статус и управление."
        >
          <Switch
            checked={draft.overlayMiniMode}
            disabled={overlayDisabled}
            onChange={(checked) => update("overlayMiniMode", checked)}
          />
        </SettingRow>
        <div className="v2-settings-action-row">
          <button
            className="v2-button"
            type="button"
            disabled={overlayDisabled || busy}
            onClick={onShowOverlayTest}
          >
            Показать тестовый overlay
          </button>
        </div>
      </div>
      <div className="v2-settings-action-row">
        <button
          className="v2-button"
          type="button"
          disabled={busy}
          onClick={onResetOverlayPosition}
        >
          Вернуть оверлей в центр экрана
        </button>
      </div>
      <SettingsStatus status={overlayStatus} />
    </SettingsCard>
  );
}
