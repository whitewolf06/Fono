import { useEffect, useState } from "react";
import {
  ipc,
  onCommandResult,
  onError,
  onKwsModelDownloaded,
  onPipelineStateChange,
  onWakeWordDetected,
  onWakeWordStatus,
} from "@/lib/ipc";
import {
  DEFAULT_SETTINGS,
  type LaunchApp,
  type PipelineState,
  type Settings as SettingsT,
  type Transcript,
  type WakeWordDiagnostics,
  type WakeWordRecognitionReport,
  type WakeWordSampleReport,
  type WakeWordTestReport,
} from "@/lib/types";
import { MicSelector } from "@/components/MicSelector";
import { MicTest } from "@/components/MicTest";
import { ModelManager } from "@/components/ModelManager";
import { amplitudeToDb, amplitudeToMeterPercent } from "@/lib/audioLevel";

const DEFAULT_CLEAN_PROMPT = `Ты — редактор голосовых транскриптов.
Задача: превратить сырой распознанный текст в читаемый, не меняя смысл.

Правила:
1. Удали слова-паразиты и запинки: «ээ», «мм», «ну», «типа», «короче», «как бы», «значит», «вот» и им подобные.
2. Исправь явные оговорки и повторы, если они мешают чтению.
3. Поставь пунктуацию и заглавные буквы в начале предложений.
4. Сохрани язык оригинала и стиль говорящего (формальный/неформальный).
5. НЕ добавляй пояснений, приветствий и прощаний.
6. Верни ТОЛЬКО готовый текст.`;

export function SettingsView() {
  const [settings, setSettings] = useState<SettingsT>(DEFAULT_SETTINGS);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [llmStatus, setLlmStatus] = useState<string | null>(null);
  const [llmTesting, setLlmTesting] = useState(false);
  const [llmModels, setLlmModels] = useState<string[]>([]);
  const [llmModelsLoading, setLlmModelsLoading] = useState(false);
  const [cleanPromptDraft, setCleanPromptDraft] = useState("");

  // Тестовая транскрипция
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<Transcript | null>(null);
  const [testDuration, setTestDuration] = useState(4000);
  const [injectMode, setInjectMode] = useState(false);
  const [pipelineState, setPipelineState] = useState<PipelineState>("idle");
  const [wakeStatus, setWakeStatus] = useState<string>("loading");
  const [wakeToggling, setWakeToggling] = useState(false);
  const [wakeDiag, setWakeDiag] = useState<WakeWordDiagnostics | null>(null);
  const [wakeTesting, setWakeTesting] = useState(false);
  const [wakeTestResult, setWakeTestResult] = useState<WakeWordTestReport | null>(null);
  const [wakeSampleRecording, setWakeSampleRecording] = useState(false);
  const [wakeSampleRecognizing, setWakeSampleRecognizing] = useState(false);
  const [wakeSample, setWakeSample] = useState<WakeWordSampleReport | null>(null);
  const [wakeRecognition, setWakeRecognition] = useState<WakeWordRecognitionReport | null>(null);
  const [kwsDownloaded, setKwsDownloaded] = useState(false);
  const [kwsDownloading, setKwsDownloading] = useState(false);
  const [logs, setLogs] = useState<string | null>(null);
  const [showLogs, setShowLogs] = useState(false);
  const [manualTranscript, setManualTranscript] = useState<Transcript | null>(null);
  const [dictationAction, setDictationAction] = useState(false);
  const [commandResult, setCommandResult] = useState<string | null>(null);

  useEffect(() => {
    ipc.getPipelineState().then(setPipelineState).catch(() => {});
    ipc.getWakeWordStatus().then((s) => setWakeStatus(s)).catch(() => {});
    ipc.isKwsModelDownloaded().then(setKwsDownloaded).catch(() => {});
    const unlistenState = onPipelineStateChange((s) => setPipelineState(s));
    const unlistenWake = onWakeWordStatus((s) => setWakeStatus(s));
    const unlistenDetected = onWakeWordDetected(() => {
      setWakeStatus("Triggered");
    });
    const unlistenKws = onKwsModelDownloaded((ok) => {
      setKwsDownloaded(ok);
      setKwsDownloading(false);
    });
    // Событие запуска можно пропустить, если окно открылось уже после него.
    // Периодический опрос берёт статус непосредственно у backend и не даёт UI
    // остаться на устаревшем «На паузе».
    const statusTimer = window.setInterval(() => {
      ipc.getWakeWordStatus().then(setWakeStatus).catch(() => {});
    }, 1000);
    return () => {
      unlistenState.then((u) => u());
      unlistenWake.then((u) => u());
      unlistenDetected.then((u) => u());
      unlistenKws.then((u) => u());
      window.clearInterval(statusTimer);
    };
  }, []);

  // Опрос runtime-диагностики wake word (уровень сигнала и последний результат).
  useEffect(() => {
    const tick = () => {
      ipc.getWakeWordDiagnostics()
        .then((d) => setWakeDiag(d))
        .catch(() => {});
    };
    tick();
    const id = setInterval(tick, 250);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    ipc.getSettings().then((s) => {
      setSettings(s);
      setCleanPromptDraft(s.clean_prompt ?? "");
    }).catch(() => setError("Не удалось загрузить настройки"));
    loadLlmModels();
    const unlistenP = onError((msg) => setError(msg));
    const unlistenCmd = onCommandResult((msg) => {
      setCommandResult(msg);
      setTimeout(() => setCommandResult(null), 4000);
    });
    return () => {
      unlistenP.then((u) => u());
      unlistenCmd.then((u) => u());
    };
  }, []);

  const loadLlmModels = async () => {
    setLlmModelsLoading(true);
    try {
      const models = await ipc.listLlmModels();
      setLlmModels(models);
    } catch (e) {
      setLlmModels([]);
    } finally {
      setLlmModelsLoading(false);
    }
  };

  const update = <K extends keyof SettingsT>(key: K, value: SettingsT[K]) => {
    setSettings((s) => ({ ...s, [key]: value }));
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await ipc.saveSettings(settings);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const testLlm = async () => {
    setLlmTesting(true);
    setLlmStatus(null);
    try {
      const msg = await ipc.testLlmConnection();
      setLlmStatus(`✓ ${msg}`);
    } catch (e) {
      setLlmStatus(`✗ ${e}`);
    } finally {
      setLlmTesting(false);
    }
  };

  const [testError, setTestError] = useState<string | null>(null);

  const runTest = async () => {
    if (!settings.whisper_model_path) {
      setTestError(
        "Сначала скачайте и выберите модель в разделе «Модель распознавания»",
      );
      return;
    }
    setTesting(true);
    setTestResult(null);
    setTestError(null);
    try {
      const result = await ipc.transcribeTest(testDuration, injectMode);
      setTestResult(result);
    } catch (e) {
      setTestError(String(e));
    } finally {
      setTesting(false);
    }
  };

  const toggleWakeWord = async () => {
    setWakeToggling(true);
    setError(null);
    try {
      // Сначала сохраняем настройки, чтобы backend применил актуальные
      // значения backend/фразы/порогов.
      await ipc.saveSettings(settings);
      if (settings.wake_word_enabled) {
        await ipc.disableWakeWord();
        setSettings((s) => ({ ...s, wake_word_enabled: false }));
        setWakeStatus("off");
      } else {
        await ipc.enableWakeWord();
        setSettings((s) => ({ ...s, wake_word_enabled: true }));
      }
    } catch (e) {
      setError(String(e));
      setWakeStatus("off");
    } finally {
      setWakeToggling(false);
    }
  };

  const runWakeWordTest = async () => {
    setWakeTesting(true);
    setWakeTestResult(null);
    setError(null);
    try {
      const result = await ipc.testWakeWordModel();
      setWakeTestResult(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setWakeTesting(false);
    }
  };

  const recordWakeWordSample = async () => {
    setWakeSampleRecording(true);
    setWakeSample(null);
    setWakeRecognition(null);
    setError(null);
    try {
      const result = await ipc.recordWakeWordSample(4000);
      setWakeSample(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setWakeSampleRecording(false);
    }
  };

  const recognizeWakeWordSample = async () => {
    setWakeSampleRecognizing(true);
    setWakeRecognition(null);
    setError(null);
    try {
      const result = await ipc.recognizeWakeWordSample();
      setWakeRecognition(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setWakeSampleRecognizing(false);
    }
  };

  const downloadKwsModel = async () => {
    setKwsDownloading(true);
    setError(null);
    try {
      await ipc.downloadKwsModel();
    } catch (e) {
      setError(String(e));
      setKwsDownloading(false);
    }
  };

  const fetchLogs = async () => {
    try {
      const l = await ipc.getRecentLogs(100);
      setLogs(l);
      setShowLogs(true);
    } catch (e) {
      setLogs(`Не удалось прочитать лог: ${e}`);
      setShowLogs(true);
    }
  };

  const canStartDictation = pipelineState === "idle" && !dictationAction;
  const canStopDictation = [
    "listening",
    "transcribing",
    "processing",
    "injecting",
  ].includes(pipelineState);

  const startDictation = async () => {
    setDictationAction(true);
    setError(null);
    try {
      await ipc.startDictation();
    } catch (e) {
      setError(String(e));
    } finally {
      setDictationAction(false);
    }
  };

  const stopDictation = async () => {
    setDictationAction(true);
    setError(null);
    try {
      const result = await ipc.stopDictation();
      setManualTranscript(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setDictationAction(false);
    }
  };

  const stateLabel: Record<PipelineState, string> = {
    idle: "Готов",
    listening: "🔴 Слушаю…",
    transcribing: "🟡 Распознаю…",
    processing: "🟡 Обрабатываю…",
    injecting: "🟢 Готово",
    error: "⛔ Ошибка",
  };

  return (
    <div className="mx-auto min-h-screen max-w-3xl px-6 py-8">
      <header className="mb-8 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Fono</h1>
          <p className="text-sm text-neutral-400">Голосовой ввод · настройки</p>
        </div>
        <button
          className="btn-primary"
          onClick={save}
          disabled={saving}
        >
          {saving ? "Сохраняю…" : "Сохранить"}
        </button>
      </header>

      {error && (
        <div className="mb-4 rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-3 text-sm text-red-300">
          {error}
        </div>
      )}

      <div className="space-y-6">
        {/* Аудио */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">🎙️ Микрофон</h2>
          <div className="space-y-4">
            <div>
              <label className="label">Устройство ввода</label>
              <MicSelector
                value={settings.audio_device_id}
                onChange={(id) => update("audio_device_id", id)}
              />
            </div>
            <MicTest deviceId={settings.audio_device_id} />
            <div>
              <label className="label">Язык распознавания</label>
              <select
                className="input"
                value={settings.language}
                onChange={(e) => update("language", e.target.value)}
              >
                <option value="auto">Авто (по речи)</option>
                <option value="ru">Русский</option>
                <option value="en">English</option>
                <option value="uk">Українська</option>
                <option value="de">Deutsch</option>
                <option value="fr">Français</option>
                <option value="es">Español</option>
              </select>
            </div>
          </div>
        </section>

        {/* Whisper-модель */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">🧠 Модель распознавания</h2>
          <ModelManager
            selectedPath={settings.whisper_model_path}
            onSelect={(p) => update("whisper_model_path", p)}
          />
          <div className="mt-4">
            <label className="flex cursor-pointer items-center gap-3">
              <input
                type="checkbox"
                className="h-4 w-4 accent-brand-500"
                checked={settings.use_gpu}
                onChange={(e) => update("use_gpu", e.target.checked)}
              />
              <span className="text-sm text-neutral-200">
                Использовать GPU (CUDA) для whisper
              </span>
            </label>
            <p className="mt-1 text-xs text-neutral-500">
              Требуется видеокарта NVIDIA и CUDA Toolkit. Перезагрузка модели
              произойдёт при следующем распознавании.
            </p>
          </div>
        </section>

        {/* Активация */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">⌨️ Активация</h2>
          <div className="space-y-4">
            <div>
              <label className="label">Горячая клавиша (push-to-talk)</label>
              <input
                className="input"
                value={settings.hotkey}
                onChange={(e) => update("hotkey", e.target.value)}
                placeholder="Ctrl+Space"
              />
              <p className="mt-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-200">
                ✅ <strong>Push-to-talk активен:</strong> зажмите{" "}
                <kbd className="rounded bg-neutral-700 px-1.5 py-0.5">
                  {settings.hotkey}
                </kbd>{" "}
                в любом приложении, наговорите текст, отпустите — текст
                автоматически вставится в активное окно.
              </p>
            </div>

            <div>
              <label className="label">Горячая клавиша голосовых команд</label>
              <input
                className="input"
                value={settings.command_hotkey}
                onChange={(e) => update("command_hotkey", e.target.value)}
                placeholder="Ctrl+Shift+Space"
              />
              <p className="mt-1 text-xs text-neutral-500">
                Зажмите{" "}
                <kbd className="rounded bg-neutral-700 px-1.5 py-0.5">
                  {settings.command_hotkey}
                </kbd>{" "}
                и скажите, например: «переключись на Telegram» или «запусти
                VS Code». Распознанная команда выполнится, а не вставится как
                текст.
              </p>
            </div>

            <div className="rounded-lg border border-neutral-700 bg-neutral-800/40 p-4">
              <div className="mb-3 flex items-center justify-between">
                <div>
                  <span className="text-sm font-medium text-neutral-200">
                    Активация по ключевой фразе
                  </span>
                  <span
                    className={`ml-2 inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs ${
                      wakeStatus === "listening"
                        ? "bg-emerald-500/20 text-emerald-300"
                        : wakeStatus === "processing"
                          ? "bg-amber-500/20 text-amber-300"
                          : wakeStatus === "loading" || wakeStatus === "missing_model"
                            ? "bg-rose-500/20 text-rose-300"
                            : wakeStatus === "paused"
                              ? "bg-neutral-600 text-neutral-300"
                              : "bg-neutral-700 text-neutral-400"
                    }`}
                  >
                    <span
                      className={`h-1.5 w-1.5 rounded-full ${
                        wakeStatus === "listening"
                          ? "animate-pulse bg-emerald-400"
                          : wakeStatus === "processing"
                            ? "animate-pulse bg-amber-400"
                            : wakeStatus === "loading" || wakeStatus === "missing_model"
                              ? "animate-pulse bg-rose-400"
                              : wakeStatus === "paused"
                                ? "bg-neutral-400"
                                : "bg-neutral-500"
                      }`}
                    />
                    {wakeStatus === "listening"
                      ? "Слушаю"
                      : wakeStatus === "processing"
                        ? "Анализирую"
                        : wakeStatus === "loading"
                          ? "Загрузка..."
                          : wakeStatus === "missing_model"
                            ? "Нет модели"
                            : wakeStatus === "paused"
                              ? "На паузе"
                              : "Выключено"}
                  </span>
                </div>
                <button
                  className={
                    settings.wake_word_enabled
                      ? "btn-secondary !px-3 !py-1 text-xs"
                      : "btn-primary !px-3 !py-1 text-xs"
                  }
                  onClick={toggleWakeWord}
                  disabled={wakeToggling}
                >
                  {wakeToggling
                    ? "..."
                    : settings.wake_word_enabled
                      ? "Выключить"
                      : "Включить"}
                </button>
              </div>

              <p className="mb-3 text-xs text-neutral-400">
                {settings.wake_backend === "sherpa_onnx" ? (
                  <>
                    Используется лёгкая модель sherpa-onnx (~3 МБ). Она быстрая,
                    но может плохо распознавать нестандартные имена и акцент.
                  </>
                ) : settings.wake_backend === "whisper_experimental" ? (
                  <>
                    Рекомендуемый точный режим: перекрывающиеся окна распознаются моделью{" "}
                    <code className="text-brand-300">{settings.wake_word_model}</code>.
                    Для «{settings.wake_word}» учитываются варианты произношения;
                    при включённом GPU обработка идёт через CUDA.
                  </>
                ) : (
                  <>Тестовый режим: срабатывание по таймеру.</>
                )}
              </p>

              <div>
                <label className="label">Backend wake word</label>
                <select
                  className="input"
                  value={settings.wake_backend}
                  onChange={(e) =>
                    update(
                      "wake_backend",
                      e.target.value as SettingsT["wake_backend"],
                    )
                  }
                  disabled={settings.wake_word_enabled}
                >
                  <option value="whisper_experimental">
                    Whisper Small + CUDA (рекомендуется)
                  </option>
                  <option value="sherpa_onnx">
                    Sherpa-ONNX (быстро, менее точно)
                  </option>
                  <option value="mock">Mock (для тестов)</option>
                </select>
              </div>

              {settings.wake_backend === "sherpa_onnx" && (
                <div className="flex items-center gap-3">
                  <button
                    type="button"
                    className="btn-primary !px-3 !py-1 text-xs"
                    onClick={downloadKwsModel}
                    disabled={kwsDownloading || kwsDownloaded}
                  >
                    {kwsDownloading
                      ? "Скачивание..."
                      : kwsDownloaded
                        ? "Модель загружена"
                        : "Скачать KWS-модель"}
                  </button>
                  <span className="text-xs text-neutral-500">
                    {kwsDownloaded
                      ? "✅ sherpa-onnx-kws-zipformer-gigaspeech"
                      : "~17 МБ, требуется для работы wake word"}
                  </span>
                </div>
              )}

              {settings.wake_backend === "whisper_experimental" && (
                <div>
                  <label className="label">Модель wake word</label>
                  <select
                    className="input"
                    value={settings.wake_word_model}
                    onChange={(e) =>
                      update(
                        "wake_word_model",
                        e.target.value as SettingsT["wake_word_model"],
                      )
                    }
                    disabled={settings.wake_word_enabled}
                  >
                    <option value="tiny">tiny (быстро, менее точно)</option>
                    <option value="base">base (баланс)</option>
                    <option value="small">small (точнее)</option>
                    <option value="medium">medium (еще точнее)</option>
                    <option value="large">large (медленно, самое точное)</option>
                  </select>
                </div>
              )}

              <div>
                <label className="label">Ключевая фраза</label>
                <div className="flex gap-2">
                  <input
                    className="input flex-1"
                    value={settings.wake_word}
                    onChange={(e) => update("wake_word", e.target.value)}
                    placeholder="okay fun"
                  />
                  <button
                    type="button"
                    className="btn-secondary whitespace-nowrap !px-3"
                    onClick={save}
                    disabled={saving || !settings.wake_word.trim()}
                  >
                    {saving ? "Применяю…" : "Применить"}
                  </button>
                </div>
                <p className="mt-1 text-xs text-neutral-500">
                  Фраза применяется без перезапуска приложения: активный detector
                  автоматически перезапустится. Для произвольных фраз используйте Whisper.
                </p>
              </div>

              <div>
                <label className="label">
                  Порог срабатывания ({settings.wake_word_threshold.toFixed(2)})
                </label>
                <input
                  type="range"
                  min={0.05}
                  max={0.95}
                  step={0.05}
                  value={settings.wake_word_threshold}
                  onChange={(e) =>
                    update("wake_word_threshold", Number(e.target.value))
                  }
                  disabled={settings.wake_word_enabled}
                  className="w-full accent-brand-500"
                />
                <p className="mt-1 text-xs text-neutral-500">
                  Меньше — чувствительнее, больше — меньше ложных срабатываний.
                </p>
              </div>

              <div>
                <label className="label">
                  Пауза до перевода ({(settings.wake_dictation_silence_ms / 1000).toFixed(1)} сек)
                </label>
                <input
                  type="range"
                  min={500}
                  max={10000}
                  step={100}
                  value={settings.wake_dictation_silence_ms}
                  onChange={(e) =>
                    update("wake_dictation_silence_ms", Number(e.target.value))
                  }
                  className="w-full accent-brand-500"
                />
                <p className="mt-1 text-xs text-neutral-500">
                  После такой паузы запись завершится и начнётся распознавание. Рекомендую 1.8–2.5 сек.
                </p>
              </div>

              <div>
                <label className="label">
                  Чувствительность окончания диктовки ({(settings.wake_dictation_speech_threshold * 100).toFixed(1)}%)
                </label>
                <input
                  type="range"
                  min={0.002}
                  max={0.03}
                  step={0.001}
                  value={settings.wake_dictation_speech_threshold}
                  onChange={(e) =>
                    update("wake_dictation_speech_threshold", Number(e.target.value))
                  }
                  className="w-full accent-brand-500"
                />
                <p className="mt-1 text-xs text-neutral-500">
                  Меньше — таймер лучше удерживается на тихой речи; если он не запускается в тишине, увеличьте значение.
                </p>
              </div>

              <div>
                <label className="label">
                  Чувствительность ({settings.wake_word_sensitivity.toFixed(2)})
                </label>
                <input
                  type="range"
                  min={0.0}
                  max={1.0}
                  step={0.05}
                  value={settings.wake_word_sensitivity}
                  onChange={(e) =>
                    update("wake_word_sensitivity", Number(e.target.value))
                  }
                  disabled={settings.wake_word_enabled}
                  className="w-full accent-brand-500"
                />
              </div>

              <div>
                <label className="label">
                  Порог начала речи / VAD ({(settings.wake_word_vad_threshold * 100).toFixed(1)}%)
                </label>
                <input
                  type="range"
                  min={0.005}
                  max={0.08}
                  step={0.005}
                  value={settings.wake_word_vad_threshold}
                  onChange={(e) =>
                    update("wake_word_vad_threshold", Number(e.target.value))
                  }
                  disabled={settings.wake_word_enabled}
                  className="w-full accent-brand-500"
                />
                <p className="mt-1 text-xs text-neutral-500">
                  Должен быть выше фонового шума, но ниже уровня обычной речи.
                </p>
              </div>

              <div className="mt-4 rounded-lg border border-neutral-700 bg-neutral-900/50 p-3">
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-xs font-medium text-neutral-300">
                    Отладка wake word
                  </span>
                  {settings.wake_backend === "sherpa_onnx" && (
                    <button
                      type="button"
                      className="btn-secondary !px-2 !py-1 text-xs"
                      onClick={runWakeWordTest}
                      disabled={wakeTesting || !kwsDownloaded}
                    >
                      {wakeTesting ? "Тест..." : "Эталон Sherpa WAV"}
                    </button>
                  )}
                </div>
                {wakeDiag ? (
                  <div className="space-y-2">
                    <div>
                      <div className="flex justify-between text-xs text-neutral-400">
                        <span>RMS</span>
                        <span>
                          {amplitudeToDb(wakeDiag.rms).toFixed(0)} dB ·{" "}
                          {(wakeDiag.rms * 100).toFixed(1)}%
                        </span>
                      </div>
                      <div className="h-2 w-full rounded bg-neutral-700">
                        <div
                          className="h-2 rounded bg-brand-500 transition-all"
                          style={{ width: `${amplitudeToMeterPercent(wakeDiag.rms)}%` }}
                        />
                      </div>
                    </div>
                    <div>
                      <div className="flex justify-between text-xs text-neutral-400">
                        <span>Peak</span>
                        <span>
                          {amplitudeToDb(wakeDiag.peak).toFixed(0)} dB ·{" "}
                          {(wakeDiag.peak * 100).toFixed(1)}%
                        </span>
                      </div>
                      <div className="h-2 w-full rounded bg-neutral-700">
                        <div
                          className="h-2 rounded bg-emerald-500 transition-all"
                          style={{ width: `${amplitudeToMeterPercent(wakeDiag.peak)}%` }}
                        />
                      </div>
                    </div>
                    <div className="grid grid-cols-2 gap-2 text-xs text-neutral-400">
                      <div>
                        Event:{" "}
                        <span className="text-neutral-200">
                          {wakeDiag.last_event || "-"}
                        </span>
                      </div>
                      <div>
                        Frames:{" "}
                        <span className="text-neutral-200">
                          {wakeDiag.frames_received}
                        </span>
                      </div>
                    </div>
                    <div className="text-xs text-neutral-400">
                      Last keyword:{" "}
                      <span className="text-neutral-200">
                        {wakeDiag.last_result_keyword || "-"}
                      </span>
                    </div>
                    {wakeDiag.last_result_json && (
                      <div className="break-all text-[10px] text-neutral-500">
                        {wakeDiag.last_result_json}
                      </div>
                    )}
                  </div>
                ) : (
                  <div className="text-xs text-neutral-500">
                    Диагностика недоступна, пока wake word выключен.
                  </div>
                )}

                <div className="mt-3 border-t border-neutral-700 pt-3">
                  <div className="mb-2 text-xs font-medium text-neutral-300">
                    Проверка вашей записи
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      className="btn-secondary !px-3 !py-1 text-xs"
                      onClick={recordWakeWordSample}
                      disabled={wakeSampleRecording || wakeSampleRecognizing}
                    >
                      {wakeSampleRecording
                        ? "Говорите сейчас…"
                        : "Записать 4 сек"}
                    </button>
                    <button
                      type="button"
                      className="btn-primary !px-3 !py-1 text-xs"
                      onClick={recognizeWakeWordSample}
                      disabled={
                        !wakeSample || wakeSampleRecording || wakeSampleRecognizing
                      }
                    >
                      {wakeSampleRecognizing
                        ? "Распознаю…"
                        : "Распознать запись"}
                    </button>
                  </div>

                  {wakeSample && (
                    <div className="mt-2 text-xs text-neutral-400">
                      Записано {(wakeSample.duration_ms / 1000).toFixed(1)} с · RMS{" "}
                      {amplitudeToDb(wakeSample.rms).toFixed(0)} dB · Peak{" "}
                      {amplitudeToDb(wakeSample.peak).toFixed(0)} dB
                    </div>
                  )}

                  {wakeRecognition && (
                    <div className="mt-2 rounded border border-neutral-700 bg-neutral-800 p-2 text-xs">
                      <div
                        className={
                          wakeRecognition.detected
                            ? "text-emerald-400"
                            : "text-rose-400"
                        }
                      >
                        {wakeRecognition.detected
                          ? "✅ Фраза совпала"
                          : "❌ Фраза не совпала"}
                      </div>
                      <div className="mt-1 text-neutral-300">
                        Модель услышала: «{wakeRecognition.recognized || "—"}»
                      </div>
                      <div className="mt-1 text-neutral-500">
                        Backend: {wakeRecognition.backend} · обработка{" "}
                        {wakeRecognition.processing_ms} мс
                      </div>
                      {wakeRecognition.json && (
                        <div className="mt-1 break-all text-[10px] text-neutral-500">
                          {wakeRecognition.json}
                        </div>
                      )}
                    </div>
                  )}
                </div>
                {wakeTestResult && (
                  <div className="mt-2 rounded border border-neutral-700 bg-neutral-800 p-2 text-xs">
                    <div
                      className={
                        wakeTestResult.detected
                          ? "text-emerald-400"
                          : "text-rose-400"
                      }
                    >
                      {wakeTestResult.detected ? "✅ Обнаружено" : "❌ Не обнаружено"}
                      {wakeTestResult.keyword
                        ? `: «${wakeTestResult.keyword}»`
                        : ""}
                    </div>
                    {wakeTestResult.json && (
                      <div className="mt-1 text-neutral-500">
                        JSON: {wakeTestResult.json}
                      </div>
                    )}
                  </div>
                )}
              </div>

              {wakeStatus === "Triggered" && (
                <div className="mt-3 rounded-lg border border-brand-500/40 bg-brand-500/10 px-4 py-3 text-sm text-brand-200">
                  🎙️ Wake word сработала! Говорите текст сейчас — запись идёт.
                </div>
              )}
            </div>
          </div>
        </section>

        {/* Оверлей */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">🪟 Оверлей</h2>
          <div className="space-y-4">
            <div>
              <label className="label">
                Масштаб ({settings.overlay_scale.toFixed(2)}x)
              </label>
              <input
                type="range"
                min={0.5}
                max={2.0}
                step={0.1}
                value={settings.overlay_scale}
                onChange={(e) =>
                  update("overlay_scale", Number(e.target.value))
                }
                className="w-full accent-brand-500"
              />
            </div>
            <div>
              <label className="label">
                Прозрачность ({Math.round(settings.overlay_opacity * 100)}%)
              </label>
              <input
                type="range"
                min={0.2}
                max={1.0}
                step={0.05}
                value={settings.overlay_opacity}
                onChange={(e) =>
                  update("overlay_opacity", Number(e.target.value))
                }
                className="w-full accent-brand-500"
              />
            </div>
            <label className="flex cursor-pointer items-center gap-3">
              <input
                type="checkbox"
                className="h-4 w-4 accent-brand-500"
                checked={settings.overlay_mini_mode}
                onChange={(e) =>
                  update("overlay_mini_mode", e.target.checked)
                }
              />
              <span className="text-sm text-neutral-200">
                Мини-режим (только индикатор)
              </span>
            </label>
          </div>
        </section>

        {/* AI-постобработка */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">✨ AI-обработка</h2>
          <div className="space-y-4">
            <div>
              <label className="label">Режим обработки</label>
              <select
                className="input"
                value={settings.ai_mode}
                onChange={(e) =>
                  update("ai_mode", e.target.value as SettingsT["ai_mode"])
                }
              >
                <option value="off">Выключено (сырой транскрипт)</option>
                <option value="clean">Чистка (ээ, мм, пунктуация)</option>
                <option value="format">Форматирование (абзацы)</option>
                <option value="command">Команды («преврати в email»)</option>
              </select>
            </div>
            <div>
              <label className="label">Провайдер LLM</label>
              <select
                className="input"
                value={settings.llm_provider}
                onChange={(e) =>
                  update(
                    "llm_provider",
                    e.target.value as SettingsT["llm_provider"],
                  )
                }
              >
                <option value="lmstudio">LM Studio (локальный)</option>
                <option value="openai">OpenAI</option>
                <option value="custom">Custom OpenAI-совместимый</option>
              </select>
            </div>
            <div>
              <label className="label">
                URL API{" "}
                {settings.llm_provider === "lmstudio"
                  ? "LM Studio"
                  : settings.llm_provider === "openai"
                    ? "OpenAI"
                    : "провайдера"}
              </label>
              <input
                className="input"
                value={settings.llm_base_url}
                onChange={(e) => update("llm_base_url", e.target.value)}
                placeholder={
                  settings.llm_provider === "openai"
                    ? "https://api.openai.com/v1"
                    : "http://localhost:1234/v1"
                }
              />
            </div>
            {settings.llm_provider !== "lmstudio" && (
              <div>
                <label className="label">API-ключ</label>
                <input
                  type="password"
                  className="input"
                  value={settings.llm_api_key ?? ""}
                  onChange={(e) =>
                    update("llm_api_key", e.target.value || null)
                  }
                  placeholder="sk-..."
                />
                <p className="mt-1 text-xs text-neutral-500">
                  Ключ хранится локально в settings.json. Для продакшена лучше
                  использовать системное хранилище.
                </p>
              </div>
            )}
            <div>
              <label className="label">Модель LLM</label>
              <div className="flex gap-2">
                <input
                  list="llm-models"
                  className="input flex-1"
                  value={settings.llm_model ?? ""}
                  onChange={(e) =>
                    update("llm_model", e.target.value || null)
                  }
                  placeholder="qwen2.5-coder-7b-instruct"
                />
                <datalist id="llm-models">
                  {llmModels.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
                <button
                  className="btn-secondary whitespace-nowrap"
                  onClick={loadLlmModels}
                  disabled={llmModelsLoading}
                  title="Обновить список моделей из LM Studio"
                >
                  {llmModelsLoading ? "…" : "Обновить"}
                </button>
              </div>
              <p className="mt-1 text-xs text-neutral-500">
                Рекомендуется: <span className="text-brand-300">qwen2.5-coder-7b-instruct</span>
              </p>
            </div>
            {settings.ai_mode === "clean" && (
              <div className="space-y-3">
                <div>
                  <label className="label">Системный промт для чистки</label>
                  <textarea
                    className="input min-h-[120px] font-mono text-xs"
                    value={cleanPromptDraft}
                    onChange={(e) => setCleanPromptDraft(e.target.value)}
                    placeholder="Оставь пустым, чтобы использовать промт по умолчанию. Или напиши свои правила для LLM: например, «убирай только „ээ“ и „мм“, сохраняй остальное»."
                  />
                  <div className="mt-2 flex items-center gap-3">
                    <button
                      className="btn-primary text-xs"
                      onClick={() => {
                        const next = { ...settings, clean_prompt: cleanPromptDraft.trim() || null };
                        setSettings(next);
                        ipc.saveSettings(next)
                          .then(() => setError(null))
                          .catch((e) => setError(String(e)));
                      }}
                    >
                      Сохранить промт
                    </button>
                    {settings.clean_prompt && (
                      <button
                        className="btn-ghost text-xs"
                        onClick={() => {
                          setCleanPromptDraft("");
                          const next = { ...settings, clean_prompt: null };
                          setSettings(next);
                          ipc.saveSettings(next).catch((e) => setError(String(e)));
                        }}
                      >
                        Сбросить на дефолт
                      </button>
                    )}
                  </div>
                </div>
                <div className="rounded-lg border border-neutral-700 bg-neutral-900/50 p-3">
                  <div className="mb-1 text-xs font-medium text-neutral-400">
                    Текущий активный промт:
                  </div>
                  <pre className="max-h-32 overflow-auto whitespace-pre-wrap text-xs text-neutral-300">
                    {settings.clean_prompt?.trim() || DEFAULT_CLEAN_PROMPT}
                  </pre>
                </div>
              </div>
            )}
            <button
              className="btn-secondary"
              onClick={testLlm}
              disabled={llmTesting}
            >
              {llmTesting ? "Проверяю…" : "Проверить соединение"}
            </button>
            {llmStatus && (
              <p className="text-sm text-neutral-300">{llmStatus}</p>
            )}
          </div>
        </section>

        {/* Голосовые команды */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">🎙️ Голосовые команды</h2>
          <p className="mb-4 text-sm text-neutral-400">
            Настройте приложения, которые можно запускать голосом. Для
            переключения на уже запущенное окно команда распознаётся
            автоматически — настраивать не нужно.
          </p>

          <div className="mb-4 rounded-lg border border-neutral-700 bg-neutral-950 p-3 text-sm text-neutral-300">
            <div className="mb-2 text-xs font-medium uppercase tracking-wider text-neutral-500">
              Доступные команды
            </div>
            <ul className="list-inside list-disc space-y-1">
              <li>
                <span className="text-brand-300">переключись на / перейди в / открой</span>{" "}
                «название окна» — активировать запущенное окно
              </li>
              <li>
                <span className="text-brand-300">запусти / старт / открыть</span>{" "}
                «приложение» — запустить программу из списка ниже
              </li>
              <li>
                <span className="text-brand-300">громче</span> — +{settings.volume_step}%
              </li>
              <li>
                <span className="text-brand-300">тише</span> — −{settings.volume_step}%
              </li>
              <li>
                <span className="text-brand-300">выключи звук / mute / без звука</span>{" "}
                — отключить звук
              </li>
              <li>
                <span className="text-brand-300">пауза / play / воспроизведение</span>{" "}
                — play/pause
              </li>
              <li>
                <span className="text-brand-300">следующий / вперёд</span> — следующий трек
              </li>
              <li>
                <span className="text-brand-300">предыдущий / назад</span> — предыдущий трек
              </li>
            </ul>
          </div>

          <div className="space-y-3">
            {settings.launch_apps.map((app, idx) => (
              <div
                key={idx}
                className="grid grid-cols-[1fr_1fr_auto] items-end gap-2"
              >
                <div>
                  <label className="label">Название</label>
                  <input
                    className="input"
                    value={app.name}
                    onChange={(e) => {
                      const next = [...settings.launch_apps];
                      next[idx] = { ...app, name: e.target.value };
                      update("launch_apps", next);
                    }}
                    placeholder="VS Code"
                  />
                </div>
                <div>
                  <label className="label">Путь к .exe</label>
                  <input
                    className="input"
                    value={app.exe_path}
                    onChange={(e) => {
                      const next = [...settings.launch_apps];
                      next[idx] = { ...app, exe_path: e.target.value };
                      update("launch_apps", next);
                    }}
                    placeholder="C:\\Program Files\\...\\Code.exe"
                  />
                </div>
                <button
                  className="btn-ghost text-xs"
                  onClick={() => {
                    const next = settings.launch_apps.filter((_, i) => i !== idx);
                    update("launch_apps", next);
                  }}
                >
                  Удалить
                </button>
              </div>
            ))}
            <button
              className="btn-secondary text-xs"
              onClick={() => {
                const next: LaunchApp[] = [
                  ...settings.launch_apps,
                  { name: "", exe_path: "", aliases: [] },
                ];
                update("launch_apps", next);
              }}
            >
              + Добавить приложение
            </button>
          </div>

          {commandResult && (
            <div className="mt-4 rounded-lg border border-brand-500/30 bg-brand-500/10 px-4 py-3 text-sm text-brand-200">
              {commandResult}
            </div>
          )}
        </section>

        {/* 🧪 Тест транскрипции */}
        <section className="card border-brand-500/30 bg-brand-500/5">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="text-lg font-medium">🧪 Тест транскрипции</h2>
            <span className="text-xs text-neutral-400">
              Статус: {stateLabel[pipelineState]}
            </span>
          </div>
          <p className="mb-4 text-sm text-neutral-400">
            Запишите фрагмент речи и проверьте, как whisper.cpp распознаёт ваш
            голос. Можно просто посмотреть результат здесь, либо включить
            «вставлять в окно» — тогда текст автоматически напечатается в
            активном окне (Notepad, Word, браузер, мессенджер).
          </p>

          <div className="mb-4 flex flex-wrap items-end gap-3">
            <div>
              <label className="label">Длительность</label>
              <select
                className="input"
                value={testDuration}
                onChange={(e) => setTestDuration(Number(e.target.value))}
                disabled={testing}
              >
                <option value={2000}>2 секунды</option>
                <option value={4000}>4 секунды</option>
                <option value={7000}>7 секунд</option>
                <option value={10000}>10 секунд</option>
              </select>
            </div>
            <label className="flex cursor-pointer items-center gap-2 pb-2 text-sm text-neutral-300">
              <input
                type="checkbox"
                className="h-4 w-4 accent-brand-500"
                checked={injectMode}
                onChange={(e) => setInjectMode(e.target.checked)}
                disabled={testing}
              />
              Вставлять в активное окно
            </label>
            <button
              className="btn-primary"
              onClick={runTest}
              disabled={testing || !settings.whisper_model_path}
            >
              {testing
                ? `🎙️ ${stateLabel[pipelineState]}`
                : "🎙️ Записать и распознать"}
            </button>
            <button
              className="btn-secondary"
              onClick={startDictation}
              disabled={!canStartDictation || dictationAction}
            >
              {dictationAction ? "..." : "Start dictation"}
            </button>
            <button
              className="btn-primary"
              onClick={stopDictation}
              disabled={!canStopDictation || dictationAction}
            >
              {dictationAction ? "..." : "Stop and insert"}
            </button>
            {!settings.whisper_model_path && (
              <span className="text-xs text-amber-400">
                Сначала выберите модель выше
              </span>
            )}
          </div>

          {manualTranscript && (
            <div className="space-y-2 rounded-lg border border-emerald-400/30 bg-emerald-500/10 p-4">
              <div className="text-xs font-medium text-neutral-400">
                Последний результат ручной диктовки:
              </div>
              <p className="whitespace-pre-wrap text-sm text-neutral-100">
                {manualTranscript.text || (
                  <span className="italic text-neutral-500">
                    (В этом запуске не было распознанного текста)
                  </span>
                )}
              </p>
            </div>
          )}

          {testing && (
            <div className="mb-4 space-y-2">
              <div className="flex items-center gap-3 rounded-lg border border-brand-500/40 bg-brand-500/10 px-4 py-3 text-sm text-brand-200">
                <span className="h-2 w-2 animate-pulse-ring rounded-full bg-brand-500" />
                {pipelineState === "listening"
                  ? `Говорите сейчас! Запись ${testDuration / 1000} сек…`
                  : "Обработка аудио…"}
              </div>
              {injectMode && pipelineState === "listening" && (
                <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-2 text-xs text-amber-200">
                  ⚠️ После распознавания текст вставится в <strong>активное
                  окно</strong>. Переключитесь сейчас в Notepad, Word, браузер
                  или мессенджер — куда хотите напечатать.
                </div>
              )}
            </div>
          )}

          {testResult && !testing && (
            <div className="space-y-2">
              <div className="text-xs text-neutral-500">
                {testResult.detected_language &&
                  `Определён язык: ${testResult.detected_language} · `}
                {testResult.text.length} символов
                {testResult.transcribe_secs != null && (
                  <>
                    {" · "}
                    <span
                      className={
                        testResult.transcribe_secs < 1.5
                          ? "text-emerald-400"
                          : testResult.transcribe_secs < 4
                            ? "text-amber-400"
                            : "text-red-400"
                      }
                    >
                      обработка {testResult.transcribe_secs.toFixed(2)}с
                    </span>
                    {" "}
                    на{" "}
                    <strong className="text-neutral-300">
                      {testResult.device ?? "CPU"}
                    </strong>
                    {testResult.audio_secs != null &&
                      testResult.audio_secs > 0 && (
                        <span className="text-neutral-500">
                          {" "}
                          (RTF ={" "}
                          {(testResult.transcribe_secs / testResult.audio_secs).toFixed(2)}
                          x)
                        </span>
                      )}
                  </>
                )}
              </div>
              <div className="rounded-lg border border-neutral-700 bg-neutral-900 p-4">
                <div className="mb-2 text-xs font-medium text-neutral-400">
                  Результат распознавания:
                </div>
                <p className="whitespace-pre-wrap text-sm text-neutral-100">
                  {testResult.text || (
                    <span className="italic text-neutral-500">
                      (распознан пустой текст — возможно, вы говорили тихо или
                      микрофон не работает)
                    </span>
                  )}
                </p>
              </div>
            </div>
          )}

          {testError && !testing && (
            <div className="mb-2 rounded-lg border border-red-500/40 bg-red-500/10 px-4 py-3">
              <div className="mb-1 text-sm font-medium text-red-300">
                ⛔ Ошибка транскрипции
              </div>
              <p className="text-sm text-red-200">{testError}</p>
              <button
                className="mt-2 text-xs text-neutral-400 underline hover:text-neutral-200"
                onClick={fetchLogs}
              >
                Показать последние строки лога →
              </button>
            </div>
          )}

          {showLogs && logs && (
            <div className="rounded-lg border border-neutral-700 bg-neutral-950 p-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-xs font-medium text-neutral-400">
                  Последние строки лога:
                </span>
                <button
                  className="text-xs text-neutral-500 hover:text-neutral-300"
                  onClick={() => setShowLogs(false)}
                >
                  скрыть ✕
                </button>
              </div>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap text-xs text-neutral-300">
                {logs}
              </pre>
            </div>
          )}
        </section>

        {/* Прочее */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">⚙️ Прочее</h2>
          <div className="space-y-4">
            <div>
              <label className="label">Способ вставки текста</label>
              <select
                className="input"
                value={settings.injection_mode}
                onChange={(e) =>
                  update(
                    "injection_mode",
                    e.target.value as SettingsT["injection_mode"],
                  )
                }
              >
                <option value="sendinput">
                  SendInput (быстро, но не везде работает)
                </option>
                <option value="clipboard">
                  Буфер обмена (Ctrl+V, работает в Telegram)
                </option>
              </select>
              <p className="mt-1 text-xs text-neutral-500">
                Если в приложении вместо текста появляется один повторяющийся
                символ — переключите на «Буфер обмена».
              </p>
            </div>
            <div>
              <label className="label">
                Шаг изменения громкости ({settings.volume_step}%)
              </label>
              <input
                type="range"
                min={2}
                max={20}
                step={2}
                value={settings.volume_step}
                onChange={(e) =>
                  update("volume_step", Number(e.target.value))
                }
                className="w-full accent-brand-500"
              />
              <p className="mt-1 text-xs text-neutral-500">
                Для команд «громче» / «тише». Стандартный шаг Windows ~2%.
              </p>
            </div>
            <label className="flex cursor-pointer items-center gap-3">
              <input
                type="checkbox"
                className="h-4 w-4 accent-brand-500"
                checked={settings.verbose_logging}
                onChange={(e) =>
                  update("verbose_logging", e.target.checked)
                }
              />
              <span className="text-sm text-neutral-200">
                Подробные логи (для отладки)
              </span>
            </label>
            <label className="flex cursor-pointer items-center gap-3">
              <input
                type="checkbox"
                className="h-4 w-4 accent-brand-500"
                checked={settings.autostart}
                onChange={(e) => update("autostart", e.target.checked)}
              />
              <span className="text-sm text-neutral-200">
                Запускать вместе с Windows
              </span>
            </label>

            <div className="flex items-center gap-3 pt-2">
              <button
                type="button"
                className="btn-secondary text-xs"
                onClick={fetchLogs}
              >
                📄 Открыть логи
              </button>
              <button
                type="button"
                className="btn-secondary text-xs"
                onClick={async () => {
                  try {
                    await ipc.clearLogs();
                    setLogs("Лог очищен.");
                    setShowLogs(true);
                  } catch (e) {
                    setError(String(e));
                  }
                }}
              >
                🧹 Очистить логи
              </button>
            </div>
          </div>
        </section>
      </div>

      <footer className="mt-8 text-center text-xs text-neutral-500">
        Fono · голосовой ввод и управление ПК · v0.1.0
      </footer>
    </div>
  );
}
