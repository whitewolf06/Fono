import { message as uiMessage } from "../../../shared/domain/messages";
import type { Preferences } from "../../../shared/domain/contracts";

/** Matches native phrase normalization: punctuation separates words; ё becomes е. */
export function normalizeWakePhrase(phrase: string): string {
  return phrase
    .toLowerCase()
    .replaceAll("ё", "е")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ");
}

export function wakePhraseLanguage(
  phrase: string,
): Preferences["wakeLanguage"] {
  if (/[а-яё]/i.test(phrase)) return "ru";
  if (/[a-z]/i.test(phrase)) return "en";
  return "ru";
}

export function validateWakePhrase(
  phrase: string,
  language: Preferences["wakeLanguage"],
): string | null {
  if (!["ru", "en"].includes(language)) return uiMessage("ui.f2146e144b");
  const normalized = normalizeWakePhrase(phrase);
  const words = normalized ? normalized.split(" ") : [];
  if (words.length < 1 || words.length > 4) return uiMessage("ui.2059fd1a33");
  const length = Array.from(normalized).length;
  if (length < 3 || length > 80) return uiMessage("ui.0326a9c56f");
  const letters = normalized.match(/\p{L}/gu) || [];
  const alphabet = language === "ru" ? /^[а-я]$/ : /^[a-z]$/;
  if (!letters.every((letter) => alphabet.test(letter)))
    return language === "ru"
      ? uiMessage("ui.df1612cd24")
      : uiMessage("ui.a085454149");
  return null;
}
