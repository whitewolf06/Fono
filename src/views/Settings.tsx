import { useEffect, useState } from "react";
import { ipc, onError, onPipelineStateChange } from "@/lib/ipc";
import {
  DEFAULT_SETTINGS,
  type PipelineState,
  type Settings as SettingsT,
  type Transcript,
} from "@/lib/types";
import { MicSelector } from "@/components/MicSelector";
import { MicTest } from "@/components/MicTest";
import { ModelManager } from "@/components/ModelManager";

export function SettingsView() {
  const [settings, setSettings] = useState<SettingsT>(DEFAULT_SETTINGS);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [llmStatus, setLlmStatus] = useState<string | null>(null);
  const [llmTesting, setLlmTesting] = useState(false);

  // Тестовая транскрипция
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<Transcript | null>(null);
  const [testDuration, setTestDuration] = useState(4000);
  const [pipelineState, setPipelineState] = useState<PipelineState>("idle");
  const [logs, setLogs] = useState<string | null>(null);
  const [showLogs, setShowLogs] = useState(false);
  const [manualTranscript, setManualTranscript] = useState<Transcript | null>(null);
  const [dictationAction, setDictationAction] = useState(false);

  useEffect(() => {
    ipc.getPipelineState().then(setPipelineState).catch(() => {});
    const unlistenState = onPipelineStateChange((s) => setPipelineState(s));
    return () => {
      unlistenState.then((u) => u());
    };
  }, []);

  useEffect(() => {
    ipc.getSettings().then(setSettings).catch(() => setError("Не удалось загрузить настройки"));
    const unlistenP = onError((msg) => setError(msg));
    return () => {
      unlistenP.then((u) => u());
    };
  }, []);

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
      const result = await ipc.transcribeTest(testDuration);
      setTestResult(result);
    } catch (e) {
      setTestError(String(e));
    } finally {
      setTesting(false);
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
          <h1 className="text-2xl font-semibold tracking-tight">WhisperClone</h1>
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
            </div>

            <label className="flex cursor-pointer items-center gap-3">
              <input
                type="checkbox"
                className="h-4 w-4 accent-brand-500"
                checked={settings.wake_word_enabled}
                onChange={(e) => update("wake_word_enabled", e.target.checked)}
              />
              <span className="text-sm text-neutral-200">
                Активация по ключевой фразе («Эй, ассистент»)
              </span>
            </label>

            {settings.wake_word_enabled && (
              <div>
                <label className="label">Ключевая фраза</label>
                <input
                  className="input"
                  value={settings.wake_word}
                  onChange={(e) => update("wake_word", e.target.value)}
                  disabled
                  placeholder="Эй, ассистент"
                />
                <p className="mt-1 text-xs text-neutral-500">
                  Кастомная фраза будет доступна в следующей версии.
                </p>
              </div>
            )}
          </div>
        </section>

        {/* AI-постобработка */}
        <section className="card">
          <h2 className="mb-4 text-lg font-medium">✨ AI-обработка (LM Studio)</h2>
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
              <label className="label">URL сервера LM Studio</label>
              <input
                className="input"
                value={settings.llm_base_url}
                onChange={(e) => update("llm_base_url", e.target.value)}
                placeholder="http://localhost:1234/v1"
              />
            </div>
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
            голос. Текст не вставляется в окна — просто отображается здесь.
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
            <div className="mb-4 flex items-center gap-3 rounded-lg border border-brand-500/40 bg-brand-500/10 px-4 py-3 text-sm text-brand-200">
              <span className="h-2 w-2 animate-pulse-ring rounded-full bg-brand-500" />
              {pipelineState === "listening"
                ? `Говорите сейчас! Запись ${testDuration / 1000} сек…`
                : "Обработка аудио…"}
            </div>
          )}

          {testResult && !testing && (
            <div className="space-y-2">
              <div className="text-xs text-neutral-500">
                {testResult.detected_language &&
                  `Определён язык: ${testResult.detected_language} · `}
                {testResult.text.length} символов
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
        </section>
      </div>

      <footer className="mt-8 text-center text-xs text-neutral-500">
        WhisperClone · локальный аналог Wispr Flow · v0.1.0
      </footer>
    </div>
  );
}
