import { message as uiMessage } from "./messages";
import type { PersonalDictionaryEntry, Preferences } from "./contracts";

export const dictionaryLimits = {
  entries: 128,
  variants: 8,
  phraseChars: 120,
  totalBytes: 32768,
} as const;

export const demoDictionaryEntries: readonly PersonalDictionaryEntry[] = [
  { written: "Fono", spoken: ["фоно"] },
  { written: "Whisper", spoken: ["виспер", "уиспер"] },
];

const normalize = (phrase: string) =>
  phrase.trim().toLowerCase().replace(/\s+/gu, " ");
const word = /^[\p{L}\p{N}\p{M}\p{Pc}\u200c\u200d]$/u;
const isWord = (character: string | undefined) =>
  character !== undefined && word.test(character);

export function validateDictionary(entries: unknown): string | null {
  if (!Array.isArray(entries) || entries.length > dictionaryLimits.entries)
    return uiMessage("ui.c6a75c96bd", { v0: String(dictionaryLimits.entries) });
  const seen = new Set<string>();
  const encoder = new TextEncoder();
  let bytes = 0;
  for (const [index, entry] of entries.entries()) {
    if (!entry || typeof entry !== "object")
      return uiMessage("ui.05ef42c17f", { v0: String(index + 1) });
    if (
      !Array.isArray(entry.spoken) ||
      !entry.spoken.length ||
      entry.spoken.length > dictionaryLimits.variants
    )
      return uiMessage("ui.9b43ba6dfc", {
        v0: String(index + 1),
        v1: String(dictionaryLimits.variants),
      });
    for (const phrase of [entry.written, ...entry.spoken]) {
      if (
        typeof phrase !== "string" ||
        !phrase.length ||
        phrase.trim() !== phrase ||
        [...phrase].length > dictionaryLimits.phraseChars ||
        /\p{Cc}/u.test(phrase) ||
        !/[\p{L}\p{N}]/u.test(phrase)
      )
        return uiMessage("ui.aaa1175aee", {
          v0: String(index + 1),
          v1: String(dictionaryLimits.phraseChars),
        });
      bytes += encoder.encode(phrase).length;
    }
    for (const phrase of entry.spoken) {
      const key = normalize(phrase);
      if (seen.has(key))
        return uiMessage("ui.1d38cefecb", { v0: String(index + 1) });
      seen.add(key);
    }
  }
  return bytes > dictionaryLimits.totalBytes
    ? uiMessage("ui.76261449e2")
    : null;
}

interface Unit {
  value: string;
  start: number;
  end: number;
}

/** Same single-pass replacement contract as native; for browser demo only. */
export function canonicalizeDictionary(
  preferences: Pick<Preferences, "dictionaryEnabled" | "dictionaryEntries">,
  text: string,
): string {
  if (
    !preferences.dictionaryEnabled ||
    !preferences.dictionaryEntries.length ||
    validateDictionary(preferences.dictionaryEntries)
  )
    return text;
  const units: Unit[] = [];
  let offset = 0;
  for (const character of text) {
    const end = offset + character.length;
    if (/\s/u.test(character)) {
      const previous = units.at(-1);
      if (previous?.value === " ") previous.end = end;
      else units.push({ value: " ", start: offset, end });
    } else
      for (const value of character.toLowerCase())
        units.push({ value, start: offset, end });
    offset = end;
  }
  const rules = preferences.dictionaryEntries
    .flatMap((entry) =>
      entry.spoken.map((spoken) => ({
        phrase: [...normalize(spoken)],
        written: entry.written,
      })),
    )
    .sort((a, b) => b.phrase.length - a.phrase.length);
  let output = "";
  let copied = 0;
  for (let cursor = 0; cursor < units.length;) {
    const start = units[cursor].start;
    const match = rules.find((rule) => {
      const endCursor = cursor + rule.phrase.length;
      if (
        endCursor > units.length ||
        units[cursor - 1]?.start === start ||
        !rule.phrase.every((value, i) => value === units[cursor + i].value)
      )
        return false;
      const end = units[endCursor - 1].end;
      return (
        !(units[endCursor] && units[endCursor].start < end) &&
        !isWord([...text.slice(Math.max(0, start - 2), start)].at(-1)) &&
        !isWord([...text.slice(end, end + 2)][0])
      );
    });
    if (match) {
      output += text.slice(copied, start) + match.written;
      cursor += match.phrase.length;
      copied = units[cursor - 1].end;
    } else cursor++;
  }
  return output + text.slice(copied);
}
