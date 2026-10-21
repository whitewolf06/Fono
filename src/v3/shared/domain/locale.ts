export type UiLocale = "ru" | "en";
export type UiLanguage = "auto" | UiLocale;
export function availableUiLanguage(value: unknown): UiLanguage {
  return value === "ru" || value === "en" ? value : "auto";
}
export function resolveUiLocale(
  language: unknown,
  systemLanguage: string,
): UiLocale {
  const choice = availableUiLanguage(language);
  return choice === "auto"
    ? /^ru(?:[-_]|$)/i.test(systemLanguage)
      ? "ru"
      : "en"
    : choice;
}
let readLocale: () => UiLocale = () => "ru";
/** Composition root supplies a reactive reader; the domain has no browser/Vue dependency. */
export function bindMessageLocale(reader: () => UiLocale): void {
  readLocale = reader;
}
export function uiLocale(): UiLocale {
  return readLocale();
}
export function uiIntlLocale(): string {
  return uiLocale() === "ru" ? "ru-RU" : "en-US";
}
