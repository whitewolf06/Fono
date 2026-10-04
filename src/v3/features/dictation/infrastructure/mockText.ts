import type {
  ProcessingPreset,
  TranslationLanguage,
} from "../../../shared/domain/processing";

export function cleanDemoText(text: string): string {
  return text
    .replace(/(^|[.!?]\s+)(Так,\s*|Ну,\s*)/g, "$1")
    .replace(/, ну,/gi, "")
    .replace(/потом, потом/gi, "потом")
    .trim()
    .replace(/^[а-яёa-z]/u, (letter) => letter.toLocaleUpperCase("ru"));
}

const translations: Record<TranslationLanguage, string> = {
  en: "Let’s keep the main things close at hand. Tomorrow, we will review the new interface and collect feedback from the team.",
  ru: "Давайте оставим главное под рукой. Завтра проверим новый интерфейс и соберём обратную связь от команды.",
  de: "Behalten wir das Wesentliche griffbereit. Morgen überprüfen wir die neue Benutzeroberfläche und sammeln Feedback vom Team.",
  fr: "Gardons l’essentiel à portée de main. Demain, nous vérifierons la nouvelle interface et recueillerons les retours de l’équipe.",
  es: "Mantengamos lo esencial a mano. Mañana revisaremos la nueva interfaz y recogeremos las opiniones del equipo.",
};

/** Browser samples demonstrate presentation; they never call a real LLM. */
export function processDemoText(
  original: string,
  preset: ProcessingPreset,
  targetLanguage: TranslationLanguage | null,
): string {
  if (preset === "raw" && !targetLanguage) return original;
  const cleaned = cleanDemoText(original);
  const text = targetLanguage ? translations[targetLanguage] : cleaned;
  const marker = targetLanguage
    ? `[Демонстрационный перевод · ${targetLanguage.toUpperCase()}]\n`
    : "";
  if (preset === "format")
    return (
      marker +
      text
        .split(/(?<=[.!?])\s+/)
        .map((sentence) => "• " + sentence)
        .join("\n")
    );
  if (preset === "task")
    return `${marker}Задача (демо)\n\n${text}\n\nРезультат: проверенный интерфейс и обратная связь команды.`;
  if (preset === "formal")
    return `${marker}Деловое письмо (демо)\n\nЗдравствуйте!\n\n${text}\n\nС уважением.`;
  return marker + text;
}
