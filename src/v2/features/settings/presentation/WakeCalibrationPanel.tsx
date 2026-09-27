import type { WakeCalibrationController } from "../application/useWakeCalibration";
import type { SettingsStatusDetail } from "../application/useSettingsDraft";
import { SettingsStatus } from "./SettingsPrimitives";

interface WakeCalibrationPanelProps {
  calibration: WakeCalibrationController;
  disabled: boolean;
}

export function WakeCalibrationPanel({
  calibration,
  disabled,
}: WakeCalibrationPanelProps) {
  const { message, recordNext, start, cancel, state, status } = calibration;
  const canRecord = status.active && !status.recording;
  const progress = `${status.accepted_samples}/${status.required_samples}`;
  const settingsStatus: SettingsStatusDetail = {
    state,
    message,
  };

  return (
    <div className="v2-settings-test-block">
      <div>
        <strong>Локальная калибровка</strong>
        <p className="v2-settings-note">
          {status.active
            ? `Прогресс: ${progress}. Каждая запись длится около 3 секунд.`
            : "10 коротких образцов помогают подобрать профиль для «рамзи»."}
        </p>
      </div>
      <div className="v2-settings-action-row">
        {!status.active && (
          <button
            className="v2-button"
            type="button"
            disabled={disabled || state === "checking"}
            onClick={start}
          >
            Начать 1/10
          </button>
        )}
        {canRecord && (
          <button
            className="v2-button v2-button--primary"
            type="button"
            disabled={disabled || state === "checking"}
            onClick={recordNext}
          >
            Записать «рамзи»
          </button>
        )}
        {status.recording && <span>Запись…</span>}
        {status.active && !status.recording && (
          <button
            className="v2-button"
            type="button"
            disabled={state === "checking"}
            onClick={cancel}
          >
            Отменить
          </button>
        )}
      </div>
      <SettingsStatus status={settingsStatus} />
    </div>
  );
}
