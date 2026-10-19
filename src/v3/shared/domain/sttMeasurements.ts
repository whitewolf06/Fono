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
    value.toLocaleString("ru-RU", {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
  return ["ru", "en"]
    .map((language) => {
      const result = model[language as "ru" | "en"];
      return result
        ? `${language.toUpperCase()}: WER ${number(result.wer * 100, 1)}% · CER ${number(result.cer * 100, 1)}% · RTF ${number(result.rtf, 3)}`
        : `${language.toUpperCase()}: не поддерживается`;
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
export const measurementContext = `Замеры CPU ${data.date}: ${data.cpu}, debug. По 10 публичных фраз RU и EN; GigaAM поддерживает только русский. Whisper: 8 потоков, GigaAM и Parakeet: 2.${data.complete ? "" : " Не все серии завершены; для остальных моделей цифр пока нет."}`;
