export interface Snippet {
  trigger: string;
  text: string;
  enabled: boolean;
}
export const snippetLimits = {
  entries: 64,
  triggerChars: 80,
  textChars: 8192,
  totalChars: 128 * 1024,
  jsonBytes: 1024 * 1024,
};
export interface SnippetTransferPort {
  import(): Promise<Snippet[] | null>;
  export(entries: Snippet[]): Promise<boolean>;
}
export function normalizeTrigger(text: string): string {
  return text
    .replace(/[\s.!…]+$/gu, "")
    .toLowerCase()
    .replaceAll("ё", "е")
    .trim()
    .split(/\s+/u)
    .join(" ");
}
const count = (text: string) => Array.from(text).length;
export function validateSnippets(entries: unknown): string | null {
  if (!Array.isArray(entries) || entries.length > snippetLimits.entries)
    return "Можно сохранить до 64 сниппетов.";
  const triggers = new Set<string>();
  let total = 0;
  for (const entry of entries) {
    if (
      !entry ||
      typeof entry !== "object" ||
      Object.keys(entry).some(
        (key) => !["trigger", "text", "enabled"].includes(key),
      ) ||
      typeof entry.trigger !== "string" ||
      typeof entry.text !== "string" ||
      typeof entry.enabled !== "boolean"
    )
      return "У сниппета нужны trigger, text и enabled.";
    const trigger = normalizeTrigger(entry.trigger);
    if (
      !trigger ||
      count(entry.trigger) > snippetLimits.triggerChars ||
      /[?\p{Cc}]/u.test(entry.trigger)
    )
      return "Триггер должен быть непустой фразой до 80 символов без вопроса и переноса строки.";
    if (triggers.has(trigger))
      return "Триггеры сниппетов должны различаться после нормализации.";
    triggers.add(trigger);
    if (
      !entry.text.trim() ||
      count(entry.text) > snippetLimits.textChars ||
      /[\p{Cc}]/u.test(entry.text.replace(/[\n\r\t]/g, ""))
    )
      return "Текст сниппета должен содержать от 1 до 8192 символов без управляющих символов.";
    total += count(entry.text) + count(entry.trigger);
  }
  return total > snippetLimits.totalChars
    ? "Общий размер сниппетов превышает 128 тысяч символов."
    : null;
}
export function parseSnippetJson(json: string): Snippet[] {
  if (new TextEncoder().encode(json).length > snippetLimits.jsonBytes)
    throw new Error("JSON-файл превышает 1 МиБ.");
  let document: unknown;
  try {
    document = JSON.parse(json.replace(/^\uFEFF/, ""));
  } catch {
    throw new Error("Некорректный JSON сниппетов.");
  }
  if (!document || typeof document !== "object" || Array.isArray(document))
    throw new Error("Нужен JSON с version: 1 и массивом snippets.");
  const value = document as Record<string, unknown>;
  if (
    value.version !== 1 ||
    Object.keys(value).some((key) => !["version", "snippets"].includes(key)) ||
    !Array.isArray(value.snippets)
  )
    throw new Error("Нужен JSON с version: 1 и массивом snippets.");
  const entries = value.snippets.map((entry) =>
    entry && typeof entry === "object"
      ? {
          ...entry,
          enabled: entry.enabled === undefined ? true : entry.enabled,
        }
      : entry,
  );
  const error = validateSnippets(entries);
  if (error) throw new Error(error);
  return entries.map((entry) => ({
    trigger: entry.trigger,
    text: entry.text,
    enabled: entry.enabled,
  }));
}
export function snippetJson(entries: Snippet[]): string {
  const error = validateSnippets(entries);
  if (error) throw new Error(error);
  return JSON.stringify({ version: 1, snippets: entries }, null, 2);
}
