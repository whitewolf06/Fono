import { uiIntlLocale } from "./locale";
import { message as uiMessage } from "./messages";
import data from "./sttMeasurements.json";
type Artifact = { filename: string; bytes: number; sha256: string };
function fingerprint(files: readonly Artifact[]): string {
  return [...files]
    .sort((a, b) => a.filename.localeCompare(b.filename, "en"))
    .map((file) => `${file.filename}:${file.bytes}:${file.sha256}`)
    .join("|");
}
function text(model: (typeof data.models)[number]): string {
  const number = (value: number, digits: number) =>
    value.toLocaleString(uiIntlLocale(), {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
  return ["ru", "en"]
    .map((language) => {
      const result = model[language as "ru" | "en"];
      return result
        ? `${language.toUpperCase()}: WER ${number(result.wer * 100, 1)}% · CER ${number(result.cer * 100, 1)}% · RTF ${number(result.rtf, 3)}`
        : uiMessage("ui.9118e239b3", { v0: String(language.toUpperCase()) });
    })
    .join("; ");
}
export function measuredModel(
  id: string,
  files: readonly Artifact[],
): string | undefined {
  const identity = fingerprint(files);
  const model = data.models.find(
    (entry) => entry.id === id && entry.fingerprint === identity,
  );
  return model ? text(model) : undefined;
}
export function demoMeasuredModel(id: string): string | undefined {
  const model = data.models.find((entry) => entry.id === id);
  return model ? text(model) : undefined;
}
export function measurementContextMessage(): string {
  return uiMessage("ui.0ff63f897f", {
    v0: String(data.date),
    v1: String(data.cpu),
    v2: String(data.complete ? "" : uiMessage("ui.80c7c985bd")),
  });
}
export const measurementContext = measurementContextMessage();
