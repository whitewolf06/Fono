import { message } from "./messages";
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
      title: message("notice.copyTitle"),
      hint: message("notice.copyHint"),
      details,
    };
  if (context.insertionBlocked || /вставк[аиу]|поле для вставки/i.test(details))
    return {
      kind: "insertion",
      title: message("notice.insertionTitle"),
      hint: message("notice.insertionHint"),
      details,
    };
  if (
    context.hasText &&
    /обработк|\bLLM\b|нейросет|модель.*недоступ/i.test(details)
  )
    return {
      kind: "processing",
      title: message("notice.processingTitle"),
      hint: message("notice.processingHint"),
      details,
    };
  return {
    kind: "other",
    title: message("notice.otherTitle"),
    hint: context.hasText
      ? message("notice.hasTextHint")
      : message("notice.otherHint"),
    details,
  };
}
