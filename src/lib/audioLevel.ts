const METER_FLOOR_DB = -60;

export function amplitudeToDb(amplitude: number): number {
  return 20 * Math.log10(Math.max(amplitude, 1e-6));
}

export function dbToMeterPercent(db: number): number {
  return Math.max(
    0,
    Math.min(100, ((db - METER_FLOOR_DB) / -METER_FLOOR_DB) * 100),
  );
}

export function amplitudeToMeterPercent(amplitude: number): number {
  return dbToMeterPercent(amplitudeToDb(amplitude));
}
