import { ruMessages } from "./ruMessages";
import ruUi from "./locales/ru.json";
import enUi from "./locales/en.json";
import { uiLocale } from "./locale";
const english = enUi as Record<string, string>;
const authoredKeys = new Map(
  Object.entries(ruUi).map(([key, value]) => [value, key]),
);
const englishKeys = new Map(
  Object.entries(enUi).map(([key, value]) => [value, key]),
);
/** Only authored dictionary entries are translated. User text is never passed here automatically. */
export function translateAuthored(value: string): string {
  const key = authoredKeys.get(value) ?? englishKeys.get(value);
  if (!key) return value;
  return uiLocale() === "en"
    ? english[key] || value
    : (ruUi as Record<string, string>)[key] || value;
}

export class MessageDictionaryError extends Error {
  constructor(
    readonly code: "missing_message" | "missing_parameter",
    readonly key: string,
  ) {
    super(`${code}: ${key}`);
    this.name = "MessageDictionaryError";
  }
}

/** Missing English entries fall back to Russian; absent keys/parameters remain diagnostic errors. */
export function createMessageDictionary<T extends Record<string, string>>(
  messages: T,
) {
  return <K extends Extract<keyof T, string>>(
    key: K,
    parameters: Record<string, string | number> = {},
  ): string => {
    if (!Object.hasOwn(messages, key))
      throw new MessageDictionaryError("missing_message", key);
    // Generated UI entries use v0/v1. Literal snippet variables remain documentation.
    const placeholders = key.startsWith("ui.")
      ? /\{(v\d+)\}/gu
      : /\{([a-zA-Z][a-zA-Z0-9_]*)\}/gu;
    return translateAuthored(messages[key]).replace(
      placeholders,
      (_, name: string) => {
        if (!Object.hasOwn(parameters, name))
          throw new MessageDictionaryError(
            "missing_parameter",
            `${key}.${name}`,
          );
        return String(parameters[name]);
      },
    );
  };
}

export const message = createMessageDictionary({ ...ruMessages, ...ruUi });
export type MessageKey = keyof typeof ruMessages | keyof typeof ruUi;
