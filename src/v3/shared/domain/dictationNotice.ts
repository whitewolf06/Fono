export interface DictationNotice {
  kind: "insertion" | "copy" | "processing" | "other";
  title: string;
  hint: string;
  details: string;
}

export function dictationNotice(
  error: string | null | undefined,
  context: { insertionBlocked?: boolean; hasText?: boolean } = {},
): DictationNotice | null {
  const details = error?.trim() || "";
  if (!details && !context.insertionBlocked) return null;
  if (
    /не удалось скопировать|ошибка копирования|буфер обмена|clipboard/i.test(
      details,
    )
  )
    return {
      kind: "copy",
      title: "Не удалось скопировать",
      hint: "Текст остаётся доступным. Попробуйте скопировать ещё раз.",
      details,
    };
  if (context.insertionBlocked || /вставк[аиу]|поле для вставки/i.test(details))
    return {
      kind: "insertion",
      title: "Вставка остановлена",
      hint: "Скопируйте текст. Перед вставкой проверьте поле: часть текста могла уже вставиться.",
      details,
    };
  if (
    context.hasText &&
    /обработк|\bLLM\b|нейросет|модель.*недоступ/i.test(details)
  )
    return {
      kind: "processing",
      title: "Не удалось обработать текст",
      hint: "Исходный текст доступен для копирования или повторной обработки.",
      details,
    };
  return {
    kind: "other",
    title: "Не удалось завершить действие",
    hint: context.hasText
      ? "Текст доступен для копирования. Причина — в подробностях."
      : "Посмотрите причину в подробностях и попробуйте ещё раз.",
    details,
  };
}
