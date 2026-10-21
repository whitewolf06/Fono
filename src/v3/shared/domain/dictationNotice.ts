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
    /не удалось скопировать|ошибка копирования|буфер обмена|could not copy|copy failed|clipboard/i.test(
      details,
    )
  )
    return {
      kind: "copy",
      get title() {
        return message("notice.copyTitle");
      },
      get hint() {
        return message("notice.copyHint");
      },
      details,
    };
  if (
    context.insertionBlocked ||
    /вставк[аиу]|поле для вставки|insert|target field/i.test(details)
  )
    return {
      kind: "insertion",
      get title() {
        return message("notice.insertionTitle");
      },
      get hint() {
        return message("notice.insertionHint");
      },
      details,
    };
  if (
    context.hasText &&
    /обработк|\bLLM\b|нейросет|модель.*недоступ|processing|model.*unavailable/i.test(
      details,
    )
  )
    return {
      kind: "processing",
      get title() {
        return message("notice.processingTitle");
      },
      get hint() {
        return message("notice.processingHint");
      },
      details,
    };
  return {
    kind: "other",
    get title() {
      return message("notice.otherTitle");
    },
    hint: context.hasText
      ? message("notice.hasTextHint")
      : message("notice.otherHint"),
    details,
  };
}
