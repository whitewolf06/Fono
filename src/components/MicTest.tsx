import { useState } from "react";
import { ipc, type MicTestResult } from "@/lib/ipc";
import { amplitudeToDb, dbToMeterPercent } from "@/lib/audioLevel";

/**
 * Записывает короткий фрагмент и показывает уровень громкости,
 * чтобы убедиться, что микрофон живой.
 *
 * `deviceId` зарезервирован для будущего использования (тест конкретного
 * устройства). Сейчас используется устройство из настроек приложения.
 */
export function MicTest({ deviceId: _deviceId }: { deviceId: string | null }) {
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<MicTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setTesting(true);
    setError(null);
    setResult(null);
    try {
      const r = await ipc.testMicrophone(2000);
      setResult(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setTesting(false);
    }
  };

  const rmsDb = result ? amplitudeToDb(result.rms) : -60;
  const peakDb = result ? amplitudeToDb(result.peak) : -60;

  let verdict: { text: string; color: string };
  if (!result) {
    verdict = { text: "", color: "" };
  } else if (result.peak < 0.005) {
    verdict = {
      text: "🔇 Микрофон молчит — звук не ловится. Проверьте подключение и права доступа.",
      color: "text-red-400",
    };
  } else if (result.rms < 0.01) {
    verdict = {
      text: "🔈 Очень тихо. Поднесите микрофон ближе или увеличьте чувствительность.",
      color: "text-amber-400",
    };
  } else if (result.peak > 0.95) {
    verdict = {
      text: "📢 Слишком громко — клиппинг. Снизьте чувствительность микрофона.",
      color: "text-amber-400",
    };
  } else {
    verdict = {
      text: "✅ Отличный уровень — микрофон работает корректно.",
      color: "text-emerald-400",
    };
  }

  return (
    <div className="rounded-lg border border-neutral-700 bg-neutral-800/40 p-4">
      <div className="mb-3 flex items-center justify-between">
        <span className="text-sm text-neutral-300">
          Проверка микрофона
        </span>
        <button
          className="btn-secondary !px-3 !py-1 text-xs"
          onClick={run}
          disabled={testing}
        >
          {testing ? "Слушаю 2 сек…" : "Тест звука"}
        </button>
      </div>

      {testing && (
        <div className="flex items-center gap-2 text-sm text-brand-300">
          <span className="h-2 w-2 animate-pulse-ring rounded-full bg-brand-500" />
          Говорите сейчас что-нибудь…
        </div>
      )}

      {result && !testing && (
        <div className="space-y-2">
          {/* Полоса уровня RMS */}
          <div>
            <div className="mb-1 flex justify-between text-xs text-neutral-400">
              <span>Средний уровень</span>
              <span>{rmsDb.toFixed(0)} dB</span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-neutral-700">
              <div
                className="h-full bg-emerald-500 transition-all"
                style={{ width: `${dbToMeterPercent(rmsDb)}%` }}
              />
            </div>
          </div>

          {/* Полоса пика */}
          <div>
            <div className="mb-1 flex justify-between text-xs text-neutral-400">
              <span>Пиковый уровень</span>
              <span>{peakDb.toFixed(0)} dB</span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-neutral-700">
              <div
                className={`h-full transition-all ${
                  result.peak > 0.95 ? "bg-red-500" : "bg-amber-500"
                }`}
                style={{ width: `${dbToMeterPercent(peakDb)}%` }}
              />
            </div>
          </div>

          <div className="text-xs text-neutral-500">
            Сэмплов: {result.samples} · длительность:{" "}
            {(result.duration_ms / 1000).toFixed(1)} с
          </div>
          <p className={`text-sm font-medium ${verdict.color}`}>
            {verdict.text}
          </p>
        </div>
      )}

      {error && !testing && (
        <p className="text-sm text-red-400">⚠ {error}</p>
      )}
    </div>
  );
}
