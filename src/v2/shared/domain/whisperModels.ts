import type { WhisperModelSize } from "@/lib/types";

interface WhisperModelMetadata {
  filename: string;
  name: string;
}

const whisperModels: Record<WhisperModelSize, WhisperModelMetadata> = {
  tiny: { filename: "ggml-tiny.bin", name: "Tiny" },
  base: { filename: "ggml-base.bin", name: "Base" },
  small: { filename: "ggml-small.bin", name: "Small" },
  medium: { filename: "ggml-medium.bin", name: "Medium" },
  large: { filename: "ggml-large-v3.bin", name: "Large v3" },
  large_turbo: {
    filename: "ggml-large-v3-turbo.bin",
    name: "Large v3 Turbo",
  },
};

export function whisperModelName(size: WhisperModelSize): string {
  return whisperModels[size].name;
}

export function whisperModelSelectionLabel(size: WhisperModelSize): string {
  return `Whisper ${whisperModelName(size)}`;
}

export function whisperModelSizeFromSelection(
  label: string,
): WhisperModelSize | undefined {
  return (Object.keys(whisperModels) as WhisperModelSize[]).find(
    (size) => whisperModelSelectionLabel(size) === label,
  );
}

export function whisperModelSizeFromPath(
  path: string | null,
): WhisperModelSize | undefined {
  const filename = path?.split(/[\\/]/).at(-1)?.toLocaleLowerCase();
  if (!filename) return undefined;

  return (Object.keys(whisperModels) as WhisperModelSize[]).find(
    (size) => whisperModels[size].filename === filename,
  );
}

export function customWhisperModelSelectionLabel(path: string | null): string {
  const filename = path?.split(/[\\/]/).at(-1) ?? "неизвестная модель";
  return `Whisper: ${filename}`;
}
