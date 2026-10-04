import type { Dictation } from "../../../shared/domain/contracts";

export interface Finding {
  title: string;
  count: number;
  example: string;
  advice: string;
}

export const findingTitles = [
  "Слова-паразиты",
  "Повторы",
  "Самоисправления",
  "Незавершённые фразы",
] as const;

const fillerWords = new Set([
  "ну",
  "вот",
  "типа",
  "короче",
  "значит",
  "ээ",
  "эм",
  "мм",
]);
const correctionPhrases = ["то есть", "вернее", "точнее", "в смысле", "ой"];

export function transcriptWords(text: string): string[] {
  return text.match(/\p{L}[\p{L}\p{M}]*/gu) ?? [];
}

export function analyze(entry: Dictation): Finding[] {
  if (entry.findings) return entry.findings;
  const text = entry.original ?? "";
  const words = transcriptWords(text);
  const normalized = words.map((word) => word.toLocaleLowerCase("ru-RU"));
  const fillers: string[] = [];
  const repeated: string[] = [];
  const corrections: string[] = [];
  for (let i = 0; i < words.length; i++) {
    if (normalized[i] === "как" && normalized[i + 1] === "бы") {
      fillers.push(words.slice(i, i + 2).join(" "));
      i++;
    } else if (fillerWords.has(normalized[i])) fillers.push(words[i]);
  }
  for (let i = 0; i < words.length; i++) {
    if (i > 0 && normalized[i] === normalized[i - 1])
      repeated.push(words.slice(i - 1, i + 1).join(" "));
    for (const phrase of correctionPhrases) {
      const length = phrase.split(" ").length;
      if (normalized.slice(i, i + length).join(" ") === phrase)
        corrections.push(words.slice(i, i + length).join(" "));
    }
  }
  const unfinished = words.length > 0 && /(?:…|\.\.\.|-)\s*$/.test(text);
  return [
    {
      title: findingTitles[0],
      count: fillers.length,
      example: fillers.join(", "),
      advice: "Попробуйте заменить привычное вводное слово короткой паузой.",
    },
    {
      title: findingTitles[1],
      count: repeated.length,
      example: repeated.join(", "),
      advice: "Сначала закончите мысль про себя, затем продолжайте фразу.",
    },
    {
      title: findingTitles[2],
      count: corrections.length,
      example: corrections.join(", "),
      advice: "Небольшая пауза поможет сформулировать мысль до её продолжения.",
    },
    {
      title: findingTitles[3],
      count: unfinished ? 1 : 0,
      example: unfinished ? words.at(-1) || "" : "",
      advice:
        "Попробуйте закончить одну мысль, прежде чем переходить к следующей.",
    },
  ];
}
