import { message as uiMessage } from "../../../shared/domain/messages";
import type {
  SettingsPort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import type { ProcessingPromptCatalog } from "../../../shared/domain/processing";
const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
export const demoPromptCatalog: ProcessingPromptCatalog = {
  maxPromptChars: 12000,
  maxTextBytes: 200000,
  presets: [
    {
      preset: "clean",
      get label() {
        return uiMessage("ui.b0f05e6f3d");
      },
      get defaultPrompt() {
        return uiMessage("ui.31fdfc5d50");
      },
    },
    {
      preset: "format",
      get label() {
        return uiMessage("ui.816655bd71");
      },
      get defaultPrompt() {
        return uiMessage("ui.0e4e8c9ad3");
      },
    },
    {
      preset: "task",
      get label() {
        return uiMessage("ui.b9ce27191d");
      },
      get defaultPrompt() {
        return uiMessage("ui.cf75298fc4");
      },
    },
    {
      preset: "formal",
      get label() {
        return uiMessage("ui.f7c565c215");
      },
      get defaultPrompt() {
        return uiMessage("ui.78b3101b3a");
      },
    },
  ],
};
export function mockProcessing(
  state: WorkspaceState,
): Pick<
  SettingsPort,
  | "processingPromptCatalog"
  | "previewProcessing"
  | "startProcessingCapture"
  | "finishProcessingCapture"
  | "cancelProcessingCapture"
> {
  let capture = false;
  return {
    async processingPromptCatalog() {
      return structuredClone(demoPromptCatalog);
    },
    async previewProcessing(input) {
      if (!input.text.trim()) throw new Error(uiMessage("ui.e352c63a71"));
      if (input.preset === "raw" && !input.targetLanguage)
        return {
          text: input.text,
          preset: input.preset,
          targetLanguage: null,
          model: null,
          elapsedMs: 0,
        };
      await wait(600);
      if (!state.aiAvailable) throw new Error(uiMessage("ui.00926201f9"));
      const cleaned = input.text.replace(/\b(ну|эээ)\b\s*/gi, "").trim();
      const text = input.targetLanguage
        ? uiMessage("ui.1117a82faa", {
            v0: String(input.targetLanguage.toUpperCase()),
            v1: String(input.text),
          })
        : input.preset === "format"
          ? cleaned
              .split(/(?<=[.!?])\s+/)
              .map((line) => `• ${line}`)
              .join("\n")
          : input.preset === "task"
            ? uiMessage("ui.9271abddce", { v0: String(cleaned) })
            : input.preset === "formal"
              ? uiMessage("ui.383f317d8c", { v0: String(cleaned) })
              : cleaned;
      return {
        text,
        preset: input.preset,
        targetLanguage: input.targetLanguage,
        model: state.preferences.processingModel,
        elapsedMs: 600,
      };
    },
    async startProcessingCapture() {
      if (!state.microphoneAvailable)
        throw new Error(uiMessage("ui.45dc01f0b5"));
      capture = true;
    },
    async finishProcessingCapture() {
      if (!capture) throw new Error(uiMessage("ui.bb285adf59"));
      capture = false;
      await wait(500);
      return "Это пробная диктовка. Давайте проверим, как модель отредактирует вопрос: когда мы обсудим новую версию?";
    },
    async cancelProcessingCapture() {
      capture = false;
    },
  };
}
