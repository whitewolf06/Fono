export const DEFAULT_RECORDING_SOUNDS_VOLUME = 25;
export function validRecordingSoundsVolume(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 100;
}
export function recordingSoundsVolume(value: unknown): number {
  return typeof value === "number" && validRecordingSoundsVolume(value)
    ? value
    : DEFAULT_RECORDING_SOUNDS_VOLUME;
}
