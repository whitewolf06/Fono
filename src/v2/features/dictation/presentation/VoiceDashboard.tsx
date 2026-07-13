import { phaseCopy } from "@/v2/shared/domain/pipeline";
import { StatusDot } from "@/v2/shared/presentation/components/StatusDot";
import { Wave } from "@/v2/shared/presentation/components/Wave";
import { useDictationDashboard } from "../application/useDictationDashboard";
import type { DictationRuntime } from "../application/dictationRuntime";

export function VoiceDashboard({ runtime }: { runtime: DictationRuntime }) {
  const { snapshot, readiness, start, stop } = useDictationDashboard(runtime);
  if (!snapshot || !readiness) return <div className="v2-loading">Подготавливаем Fono…</div>;

  const copy = phaseCopy[snapshot.phase];
  const isListening = snapshot.phase === "listening";
  const isBusy = snapshot.phase !== "idle" && !isListening;

  return (
    <div className="v2-dashboard">
      <header className="v2-page-header">
        <div>
          <p className="v2-kicker">Голосовой ввод</p>
          <h1>Говорите — Fono сделает остальное</h1>
        </div>
        <span className="v2-local-chip"><StatusDot />Локальная обработка</span>
      </header>

      <section className={`v2-voice-stage v2-voice-stage--${snapshot.phase}`}>
        <button className="v2-state-pill" type="button" onClick={isListening ? stop : start} disabled={isBusy}>
          <StatusDot tone={isListening ? "active" : "ready"} />
          {copy.label}
          <span className="v2-mini-bars" aria-hidden="true"><i /><i /><i /><i /></span>
        </button>
        <Wave />
        <div className="v2-spirit" role="img" aria-label="Огонёк Fono" />
        <div className="v2-pond" aria-hidden="true" />
        <p className="v2-stage-caption">{copy.hint}</p>
        <button className="v2-hotkey" type="button" onClick={isListening ? stop : start} disabled={isBusy}>
          <span>⌨</span>{isListening ? "Закончить диктовку" : snapshot.hotkey}
        </button>
      </section>

      <section className="v2-transcript-card" aria-live="polite">
        <div className="v2-transcript-card__top"><span>Текущая диктовка</span><span>{snapshot.language}</span></div>
        <p>{snapshot.transcript || (isListening ? "Слушаю вас…" : "Нажмите кнопку и начните говорить.")}{isListening && <i className="v2-caret" />}</p>
        <div className="v2-transcript-card__bottom"><span>Текст остаётся на этом устройстве</span><span>{copy.label}</span></div>
      </section>

      <section className="v2-readiness-grid" aria-label="Готовность Fono">
        <ReadinessCard title="Микрофон" value={readiness.microphone === "ready" ? "Подключён" : "Требует проверки"} tone={readiness.microphone === "ready" ? "ready" : "muted"} />
        <ReadinessCard title="Модель" value={readiness.model === "ready" ? "Готова к работе" : "Не выбрана"} tone={readiness.model === "ready" ? "ready" : "muted"} />
        <ReadinessCard title="Wake word" value={readiness.wakeWord === "active" ? "Активен" : "Выключен"} tone={readiness.wakeWord === "active" ? "active" : "muted"} />
      </section>
    </div>
  );
}

function ReadinessCard({ title, value, tone }: { title: string; value: string; tone: "ready" | "active" | "muted" }) {
  return <article className="v2-readiness-card"><span>{title}</span><strong><StatusDot tone={tone} />{value}</strong></article>;
}
