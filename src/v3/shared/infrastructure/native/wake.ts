import { call } from "./ipc";
import type { NativeContext } from "./context";
import type {
  WakeCalibrationStatus,
  WakeProfileValidationStatus,
} from "../../../../lib/types";
export function nativeWake(ctx: NativeContext) {
  return {
    async action(action: string) {
      if (action === "download") {
        if (!(await call<boolean>("is_kws_model_downloaded")))
          await call("download_kws_model");
        return "Модель пробуждения готова.";
      }
      if (action === "test") {
        await call("record_wake_word_sample", { durationMs: 4000 });
        const result = await call<{ detected: boolean; recognized: string }>(
          "recognize_wake_word_sample",
        );
        return result.detected
          ? "Фраза обнаружена: " + result.recognized
          : "Фраза не обнаружена. Проверьте микрофон и повторите.";
      }
      if (["start", "record", "cancel"].includes(action)) {
        const command = {
          start: "start_wake_calibration",
          record: "record_wake_calibration_sample",
          cancel: "cancel_wake_calibration",
        }[action]!;
        const result = await call<WakeCalibrationStatus>(command);
        await ctx.readSettings();
        return result.active
          ? "Принято " +
              result.accepted_samples +
              " из " +
              result.required_samples +
              ". " +
              (result.latest_result?.accepted === false
                ? "Образец отклонён: " + result.latest_result.reason
                : "Запишите следующий образец.")
          : action === "cancel"
            ? "Калибровка отменена."
            : "Калибровка завершена. Перейдите к проверке профиля.";
      }
      const result = await call<WakeProfileValidationStatus>(
        action === "validate"
          ? "start_wake_profile_validation"
          : "record_wake_profile_validation_sample",
        action === "validate" ? undefined : { kind: action },
      );
      await ctx.readSettings();
      return result.completed
        ? "Проверка завершена. Можно включить пробуждение."
        : result.failed
          ? "Профиль не прошёл проверку. Повторите калибровку."
          : "Фраза: " +
            result.positive_passed +
            "/" +
            result.positive_required +
            ". Тишина: " +
            (result.silence_passed ? "пройдена" : "ожидается") +
            ". Другая фраза: " +
            (result.other_phrase_passed ? "пройдена" : "ожидается") +
            ".";
    },
  };
}
