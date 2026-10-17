import { ruMessages } from "./ruMessages";

export class MessageDictionaryError extends Error {
  constructor(
    readonly code: "missing_message" | "missing_parameter",
    readonly key: string,
  ) {
    super(`${code}: ${key}`);
    this.name = "MessageDictionaryError";
  }
}

/** A small dictionary boundary; language selection and English belong to F-10. */
export function createMessageDictionary<T extends Record<string, string>>(
  messages: T,
) {
  return <K extends Extract<keyof T, string>>(
    key: K,
    parameters: Record<string, string | number> = {},
  ): string => {
    if (!Object.hasOwn(messages, key))
      throw new MessageDictionaryError("missing_message", key);
    return messages[key].replace(
      /\{([a-zA-Z][a-zA-Z0-9_]*)\}/gu,
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

export const message = createMessageDictionary(ruMessages);
export type MessageKey = keyof typeof ruMessages;
