import { message as uiMessage } from "../../../shared/domain/messages";
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
    return uiMessage("ui.7f85fae5a1");
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
      return uiMessage("ui.125163b722");
    const trigger = normalizeTrigger(entry.trigger);
    if (
      !trigger ||
      count(entry.trigger) > snippetLimits.triggerChars ||
      /[?\p{Cc}]/u.test(entry.trigger)
    )
      return uiMessage("ui.6e5de94ebd");
    if (triggers.has(trigger)) return uiMessage("ui.d37d5edb40");
    triggers.add(trigger);
    if (
      !entry.text.trim() ||
      count(entry.text) > snippetLimits.textChars ||
      /[\p{Cc}]/u.test(entry.text.replace(/[\n\r\t]/g, ""))
    )
      return uiMessage("ui.79ba2e1a4c");
    total += count(entry.text) + count(entry.trigger);
  }
  return total > snippetLimits.totalChars ? uiMessage("ui.685ba22547") : null;
}
export function parseSnippetJson(json: string): Snippet[] {
  if (new TextEncoder().encode(json).length > snippetLimits.jsonBytes)
    throw new Error(uiMessage("ui.7c128e2dc5"));
  let document: unknown;
  try {
    document = JSON.parse(json.replace(/^\uFEFF/, ""));
  } catch {
    throw new Error(uiMessage("ui.f7886f874e"));
  }
  if (!document || typeof document !== "object" || Array.isArray(document))
    throw new Error(uiMessage("ui.4a001d0b4c"));
  const value = document as Record<string, unknown>;
  if (
    value.version !== 1 ||
    Object.keys(value).some((key) => !["version", "snippets"].includes(key)) ||
    !Array.isArray(value.snippets)
  )
    throw new Error(uiMessage("ui.4a001d0b4c"));
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
