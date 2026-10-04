import type { SettingsPort } from "../../../shared/domain/contracts";
import type {
  ProcessingPromptCatalog,
  ProcessingPreviewResult,
} from "../../../shared/domain/processing";
import { call } from "../../../shared/infrastructure/native/ipc";
type ProcessingMethods = Pick<
  SettingsPort,
  | "processingPromptCatalog"
  | "previewProcessing"
  | "startProcessingCapture"
  | "finishProcessingCapture"
  | "cancelProcessingCapture"
>;
export function nativeProcessing(): ProcessingMethods {
  let sessionId: number | null = null;
  let starting: Promise<number> | null = null;
  return {
    async processingPromptCatalog() {
      const result = await call<{
        presets: {
          preset: ProcessingPromptCatalog["presets"][number]["preset"];
          label: string;
          default_prompt: string;
        }[];
        max_prompt_chars: number;
        max_text_bytes: number;
      }>("get_processing_prompt_catalog");
      return {
        presets: result.presets.map((item) => ({
          preset: item.preset,
          label: item.label,
          defaultPrompt: item.default_prompt,
        })),
        maxPromptChars: result.max_prompt_chars,
        maxTextBytes: result.max_text_bytes,
      };
    },
    async previewProcessing(input) {
      const result = await call<{
        text: string;
        model: string | null;
        elapsed_ms: number;
        preset: ProcessingPreviewResult["preset"];
        target_language: ProcessingPreviewResult["targetLanguage"];
      }>("preview_processing_text", {
        input: {
          text: input.text,
          preset: input.preset,
          targetLanguage: input.targetLanguage,
          ...(input.promptOverride
            ? {
                promptOverride: {
                  use_custom: input.promptOverride.useCustom,
                  custom_prompt: input.promptOverride.customPrompt,
                },
              }
            : {}),
        },
      });
      return {
        text: result.text,
        model: result.model,
        elapsedMs: result.elapsed_ms,
        preset: result.preset,
        targetLanguage: result.target_language,
      };
    },
    async startProcessingCapture() {
      if (sessionId !== null || starting)
        throw new Error("Тестовая диктовка уже запущена.");
      starting = call<{ sessionId: number }>(
        "start_processing_test_capture",
      ).then((result) => {
        sessionId = result.sessionId;
        return result.sessionId;
      });
      try {
        await starting;
      } finally {
        starting = null;
      }
    },
    async finishProcessingCapture() {
      await starting;
      if (sessionId === null) throw new Error("Тестовая диктовка не запущена.");
      const owned = sessionId;
      try {
        return (
          await call<{ text: string }>("finish_processing_test_capture", {
            sessionId: owned,
          })
        ).text;
      } finally {
        if (sessionId === owned) sessionId = null;
      }
    },
    async cancelProcessingCapture() {
      const pendingStart = starting;
      const previousSession = sessionId;
      const owned = pendingStart
        ? await pendingStart.catch(() => null)
        : previousSession;
      if (owned === null) return;
      if (sessionId === owned) sessionId = null;
      await call("cancel_processing_test_capture", { sessionId: owned });
    },
  };
}
