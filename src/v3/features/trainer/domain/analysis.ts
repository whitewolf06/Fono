import type { Dictation } from "../../../shared/domain/contracts";
export interface Finding {
  title: string;
  count: number;
  example: string;
  advice: string;
}
export function analyze(entry: Dictation): Finding[] {
  const text = entry.original ?? "";
  const fillers =
    text.match(/(?:^|[\s,])(ну|то есть|в общем)(?=[\s,.!?]|$)/gi) ?? [];
  const repeated = text.match(/([а-яё]+),?\s+\1(?=[\s,.!?]|$)/gi) ?? [];
  const uncertain = text.match(/мне кажется|как бы/gi) ?? [];
  return [
    {
      title: "Слова-паразиты",
      count: fillers.length,
      example: fillers.map((v) => v.trim()).join(", "),
      advice: "Попробуйте заменить привычное вводное слово короткой паузой.",
    },
    {
      title: "Повторы",
      count: repeated.length,
      example: repeated.join(", "),
      advice: "Сначала закончите мысль про себя, затем продолжайте фразу.",
    },
    {
      title: "Неуверенные обороты",
      count: uncertain.length,
      example: uncertain.join(", "),
      advice:
        "Когда мысль ясна, говорите её прямо. Оставляйте оговорки там, где они нужны.",
    },
  ];
}
