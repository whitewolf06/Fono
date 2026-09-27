import type { WakeCalibrationController } from "../application/useWakeCalibration";
import type { WakeProfileValidationController } from "../application/useWakeProfileValidation";
import type { SettingsStatusDetail } from "../application/useSettingsDraft";
import { SettingsStatus } from "./SettingsPrimitives";

interface WakeProfileValidationPanelProps {
  calibration: WakeCalibrationController;
  validation: WakeProfileValidationController;
  wakeWordEnabled: boolean;
  onActivateWakeWord: () => void;
}

export function WakeProfileValidationPanel({
  calibration,
  validation,
  wakeWordEnabled,
  onActivateWakeWord,
}: WakeProfileValidationPanelProps) {
  const profile = calibration.status.profile;
  const { message, record, start, state, status } = validation;
  if (!profile) return null;

  const completed = status.completed;
  const settingsStatus: SettingsStatusDetail = { state, message };
  const nextCheck = nextValidationCheck(status);

  return (
    <div className="v2-settings-test-block">
      <div>
        <strong>Проверка перед включением</strong>
        <p className="v2-settings-note">
          {completed
            ? "Новые образцы прошли локальную проверку. Проценты точности не рассчитываются."
            : "Три новых «рамзи», затем тишина и другая короткая фраза. Аудио не сохраняется."}
        </p>
      </div>
      <div className="v2-settings-action-row">
        {!completed && !status.active && (
          <button
            className="v2-button"
            type="button"
            disabled={state === "checking"}
            onClick={start}
          >
            {status.failed ? "Проверить заново" : "Проверить профиль"}
          </button>
        )}
        {nextCheck && (
          <button
            className="v2-button v2-button--primary"
            type="button"
            disabled={state === "checking"}
            onClick={() => record(nextCheck.kind)}
          >
            {nextCheck.label}
          </button>
        )}
        {status.recording && <span>Проверка…</span>}
        {completed && !wakeWordEnabled && (
          <button
            className="v2-button v2-button--primary"
            type="button"
            disabled={state === "checking"}
            onClick={onActivateWakeWord}
          >
            Включить wake word
          </button>
        )}
      </div>
      <SettingsStatus status={settingsStatus} />
    </div>
  );
}

function nextValidationCheck(status: WakeProfileValidationController["status"]) {
  if (!status.active || status.recording) return null;
  if (status.positive_passed < status.positive_required) {
    return { kind: "positive" as const, label: "Произнесите «рамзи»" };
  }
  if (!status.silence_passed) {
    return { kind: "silence" as const, label: "Проверить тишину" };
  }
  if (!status.other_phrase_passed) {
    return { kind: "other_phrase" as const, label: "Другая короткая фраза" };
  }
  return null;
}
