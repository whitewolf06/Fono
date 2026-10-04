import type {
  ProcessingPrompts,
  PromptPreset,
} from "../../../shared/domain/processing";
export const promptPresets: PromptPreset[] = [
  "clean",
  "format",
  "task",
  "formal",
];
export function emptyProcessingPrompts(): ProcessingPrompts {
  return Object.fromEntries(
    promptPresets.map((preset) => [
      preset,
      { useCustom: false, customPrompt: "" },
    ]),
  ) as ProcessingPrompts;
}
export function validateProcessingPrompts(
  prompts: ProcessingPrompts,
): string | null {
  for (const preset of promptPresets) {
    const choice = prompts?.[preset];
    if (
      !choice ||
      typeof choice.useCustom !== "boolean" ||
      typeof choice.customPrompt !== "string"
    )
      return "Проверьте системные промпты обработки.";
    if ([...choice.customPrompt].length > 12000)
      return "Системный промпт не должен превышать 12 000 символов.";
    if (choice.useCustom && !choice.customPrompt.trim())
      return "Введите свой системный промпт или выберите встроенный.";
  }
  return null;
}
export function clonePreferences<T>(value: T): T {
  // Vue reactive proxies cannot be passed to structuredClone.
  return JSON.parse(JSON.stringify(value)) as T;
}
export function equalPreference(a: unknown, b: unknown): boolean {
  return typeof a === "object" || typeof b === "object"
    ? JSON.stringify(a) === JSON.stringify(b)
    : a === b;
}
