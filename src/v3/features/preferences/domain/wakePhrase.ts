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
  if (!["ru", "en"].includes(language))
    return "Выберите русский или английский язык пробуждения.";
  const normalized = normalizeWakePhrase(phrase);
  const words = normalized ? normalized.split(" ") : [];
  if (words.length < 1 || words.length > 4)
    return "Фраза пробуждения должна содержать от 1 до 4 слов.";
  const length = Array.from(normalized).length;
  if (length < 3 || length > 80)
    return "Укажите фразу пробуждения длиной от 3 до 80 символов.";
  const letters = normalized.match(/\p{L}/gu) || [];
  const alphabet = language === "ru" ? /^[а-я]$/ : /^[a-z]$/;
  if (!letters.every((letter) => alphabet.test(letter)))
    return language === "ru"
      ? "Для русского пробуждения используйте кириллицу, например «Эй, фоно»."
      : "Для английского пробуждения используйте латиницу, например «Hey, Fono».";
  return null;
}
