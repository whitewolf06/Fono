import { call } from "./ipc";
import type { NativeContext } from "./context";
import type { WakePort } from "../../domain/wake";
import type {
  WakeCalibrationStatus,
  WakeProfileValidationStatus,
  WakeWordRecognitionReport,
  WakeWordCapabilities,
} from "../../../../lib/types";

export function nativeWake(ctx: NativeContext): WakePort {
  async function load() {
    const [setup, validation, modelReady, caps] = await Promise.all([
      call<WakeCalibrationStatus>("get_wake_calibration_status"),
      call<WakeProfileValidationStatus>("get_wake_profile_validation_status"),
      call<boolean>("is_kws_model_downloaded"),
      call<WakeWordCapabilities>("get_wake_word_capabilities"),
    ]);
    const current = await ctx.readSettings();
    const profileReady =
      !!setup.profile &&
      setup.profile.backend === current.wake_backend &&
      setup.profile.phrase.toLocaleLowerCase().trim() ===
        current.wake_word.toLocaleLowerCase().trim();
    ctx.state.wakeCapabilities = {
      backend: caps.backend,
      customPhrase: caps.supports_custom_phrase,
      languages: (caps.available_languages || ["ru", "en"]).map((value) => ({
        value,
        label: value === "ru" ? "Русский" : "Английский",
      })),
    };
    ctx.state.wakeSetup = {
      modelReady,
      profileReady,
      verified: profileReady && !!setup.profile?.validation,
      active: setup.active,
      required: setup.required_samples,
      accepted: setup.accepted_samples,
      rejected: setup.rejected_samples,
      phrase: setup.phrase,
      latest: setup.latest_result
        ? {
            accepted: setup.latest_result.accepted,
            reason: setup.latest_result.reason || undefined,
            detected: setup.latest_result.detected,
          }
        : undefined,
      validation: {
        active: validation.active,
        completed: validation.completed,
        failed: validation.failed,
        positivePassed: validation.positive_passed,
        positiveRequired: validation.positive_required,
        silencePassed: validation.silence_passed,
        otherPhrasePassed: validation.other_phrase_passed,
      },
    };
  }
  async function execute(command: string, args?: Record<string, unknown>) {
    await call(command, args);
    await load();
  }
  return {
    load,
    async download() {
      if (!(await call<boolean>("is_kws_model_downloaded")))
        await call("download_kws_model");
      await load();
    },
    async test() {
      await call("record_wake_word_sample", { durationMs: 4000 });
      const result = await call<WakeWordRecognitionReport>(
        "recognize_wake_word_sample",
      );
      return result.detected
        ? "Фраза обнаружена: «" +
            result.recognized +
            "». Проверка заняла " +
            result.processing_ms +
            " мс."
        : "Фраза не обнаружена. Проверьте микрофон или настройте профиль заново.";
    },
    begin: () => execute("start_wake_calibration"),
    record: () => execute("record_wake_calibration_sample"),
    beginValidation: () => execute("start_wake_profile_validation"),
    validate: (kind) =>
      execute("record_wake_profile_validation_sample", { kind }),
    cancel: () => execute("cancel_wake_calibration"),
  };
}
