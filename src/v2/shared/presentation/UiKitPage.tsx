import { useState, type CSSProperties } from "react";
import { AppIcon } from "./components/AppIcon";
import { StatusDot } from "./components/StatusDot";
import { StatusChip } from "./components/StatusChip";
import { DevKitExtensions } from "./DevKitExtensions";

export function UiKitPage() {
  const [enabled, setEnabled] = useState(true);
  const [range, setRange] = useState(60);
  const [selected, setSelected] = useState("auto");
  const [delivery, setDelivery] = useState("sendinput");
  const [notifications, setNotifications] = useState(true);
  const [activeTab, setActiveTab] = useState("overview");
  const [openSection, setOpenSection] = useState("advanced");

  return (
    <div className="v2-kit-page">
      <header className="v2-page-header">
        <div>
          <p className="v2-kicker">Только для разработки</p>
          <h1>UI kit</h1>
        </div>
        <span className="v2-dev-chip">mock only</span>
      </header>
      <p className="v2-kit-lead">
        Базовые примитивы для экранов Fono. Изменения здесь не сохраняются и не
        вызывают Tauri.
      </p>

      <div className="v2-kit-grid">
        <KitSection title="Типографика" wide>
          <div className="v2-type-specimen">
            <p className="v2-display">Fono слушает.</p>
            <h1>Заголовок страницы</h1>
            <h2>Заголовок секции</h2>
            <h3>Заголовок карточки</h3>
            <p>
              Основной текст помогает быстро понять, что произойдёт с диктовкой
              и где изменить нужный параметр.
            </p>
            <p className="v2-body-small">
              Вторичный текст — для пояснений, статусов и безопасных подсказок.
            </p>
            <p className="v2-caption">CAPTION · 11 PX · SEMIBOLD</p>
          </div>
        </KitSection>
        <KitSection title="Цвет и статус">
          <div className="v2-kit-statuses">
            <StatusExample label="Готово" tone="ready" />
            <StatusExample label="Активно" tone="active" />
            <StatusExample label="Недоступно" tone="muted" />
          </div>
          <div className="v2-kit-swatches">
            <i />
            <i />
            <i />
            <i />
            <i />
          </div>
        </KitSection>

        <KitSection title="Кнопки">
          <div className="v2-glow-button-demo">
            <button className="v2-glow-outline-button" type="button">
              <MicIcon />
              <span>Голос</span>
            </button>
            <p>
              Наведите курсор: блик пройдёт по контуру, не подсвечивая всю
              кнопку целиком.
            </p>
          </div>
          <div className="v2-kit-row">
            <button className="v2-button v2-button--primary">
              Начать диктовку
            </button>
            <button className="v2-button">Отменить</button>
            <button className="v2-button v2-button--ghost">Подробнее →</button>
          </div>
          <div className="v2-kit-row">
            <button className="v2-button v2-button--primary" disabled>
              Загрузка…
            </button>
            <button className="v2-icon-button" aria-label="Настройки">
              ⚙
            </button>
            <button className="v2-icon-button" aria-label="Остановить">
              ■
            </button>
          </div>
        </KitSection>

        <KitSection title="Поля и выбор">
          <label className="v2-field">
            <span>Ключевая фраза</span>
            <input defaultValue="okay fun" />
          </label>
          <label className="v2-field">
            <span>Ускорение</span>
            <select
              value={selected}
              onChange={(event) => setSelected(event.target.value)}
            >
              <option value="auto">Авто (рекомендуется)</option>
              <option value="cuda">CUDA</option>
              <option value="vulkan">Vulkan</option>
              <option value="cpu">CPU</option>
            </select>
          </label>
          <label className="v2-field">
            <span>Подсказка для AI</span>
            <textarea
              defaultValue="Сохраняй смысл речи и исправляй только очевидные оговорки."
              rows={3}
            />
          </label>
        </KitSection>

        <KitSection title="Выбор и переключатели">
          <div className="v2-kit-setting">
            <div>
              <strong>Wake word</strong>
              <span>Слушать ключевую фразу в фоне</span>
            </div>
            <button
              className={`v2-switch ${enabled ? "is-on" : ""}`}
              type="button"
              onClick={() => setEnabled(!enabled)}
              aria-pressed={enabled}
            >
              <i />
            </button>
          </div>
          <label className="v2-check">
            <input
              type="checkbox"
              checked={notifications}
              onChange={(event) => setNotifications(event.target.checked)}
            />
            <span aria-hidden="true">✓</span>Показывать уведомление о готовом
            тексте
          </label>
          <fieldset className="v2-radio-group">
            <legend>Способ вставки</legend>
            <label className="v2-radio">
              <input
                type="radio"
                name="delivery"
                value="sendinput"
                checked={delivery === "sendinput"}
                onChange={(event) => setDelivery(event.target.value)}
              />
              <span aria-hidden="true" />
              SendInput
            </label>
            <label className="v2-radio">
              <input
                type="radio"
                name="delivery"
                value="clipboard"
                checked={delivery === "clipboard"}
                onChange={(event) => setDelivery(event.target.value)}
              />
              <span aria-hidden="true" />
              Буфер обмена
            </label>
          </fieldset>
          <label className="v2-field v2-field--range">
            <span>
              Чувствительность <b>{range}%</b>
            </span>
            <input
              type="range"
              min="0"
              max="100"
              value={range}
              style={{ "--range": `${range}%` } as CSSProperties}
              onChange={(event) => setRange(Number(event.target.value))}
            />
          </label>
        </KitSection>

        <KitSection title="Карточки">
          <div className="v2-kit-card-grid">
            <article className="v2-kit-info-card">
              <AppIcon />
              <div>
                <span>Модель</span>
                <strong>Whisper Small</strong>
                <small>Готова к работе</small>
              </div>
            </article>
            <article className="v2-kit-info-card">
              <StatusDot tone="active" />
              <div>
                <span>Wake word</span>
                <strong>Активен</strong>
                <small>okay fun</small>
              </div>
            </article>
          </div>
        </KitSection>

        <KitSection title="Обратная связь">
          <div className="v2-alert v2-alert--success">
            <StatusDot />
            Модель загружена и готова к диктовке.
          </div>
          <div className="v2-alert v2-alert--warning">
            Проверьте микрофон перед первым запуском.
          </div>
          <div className="v2-alert v2-alert--error">
            Не удалось подключиться к LM Studio.
          </div>
        </KitSection>

        <KitSection title="Tabs и карточки" wide>
          <div className="v2-tabs" role="tablist" aria-label="Пример вкладок">
            <button
              className={activeTab === "overview" ? "is-active" : ""}
              type="button"
              role="tab"
              aria-selected={activeTab === "overview"}
              onClick={() => setActiveTab("overview")}
            >
              Обзор
            </button>
            <button
              className={activeTab === "sound" ? "is-active" : ""}
              type="button"
              role="tab"
              aria-selected={activeTab === "sound"}
              onClick={() => setActiveTab("sound")}
            >
              Аудио
            </button>
            <button
              className={activeTab === "privacy" ? "is-active" : ""}
              type="button"
              role="tab"
              aria-selected={activeTab === "privacy"}
              onClick={() => setActiveTab("privacy")}
            >
              Приватность
            </button>
          </div>
          <article className="v2-tab-card">
            <span className="v2-kicker">{activeTab}</span>
            <strong>
              {activeTab === "overview"
                ? "Fono готов к работе"
                : activeTab === "sound"
                  ? "Микрофон выбран"
                  : "Обработка остаётся локальной"}
            </strong>
            <p>
              {activeTab === "overview"
                ? "Карточка с вкладками подходит для компактного обзора состояния."
                : activeTab === "sound"
                  ? "Здесь будут устройство ввода, проверка сигнала и язык распознавания."
                  : "Эта вкладка объясняет, куда отправляется аудио и текст."}
            </p>
          </article>
        </KitSection>

        <KitSection title="Accordion и расширенные параметры" wide>
          <div className="v2-accordion">
            <button
              className="v2-accordion__trigger"
              type="button"
              aria-expanded={openSection === "advanced"}
              onClick={() =>
                setOpenSection(openSection === "advanced" ? "" : "advanced")
              }
            >
              <span>
                <b>Расширенные параметры wake word</b>
                <small>Порог, VAD и поведение после паузы</small>
              </span>
              <i>{openSection === "advanced" ? "−" : "+"}</i>
            </button>
            {openSection === "advanced" && (
              <div className="v2-accordion__content">
                <div className="v2-advanced-card">
                  <span>Порог срабатывания</span>
                  <strong>0.25</strong>
                  <small>
                    Меньше — чувствительнее, больше — меньше ложных
                    срабатываний.
                  </small>
                </div>
                <div className="v2-advanced-card">
                  <span>Пауза до распознавания</span>
                  <strong>2.0 с</strong>
                  <small>
                    Показывается таймером в overlay во время wake-диктовки.
                  </small>
                </div>
              </div>
            )}
          </div>
          <div className="v2-card-variants">
            <article className="v2-feature-card">
              <span className="v2-feature-card__icon">◉</span>
              <div>
                <strong>Базовая карточка</strong>
                <p>Короткая настройка с понятным действием.</p>
              </div>
              <button className="v2-button">Настроить</button>
            </article>
            <article className="v2-feature-card v2-feature-card--emphasis">
              <span className="v2-feature-card__icon">✦</span>
              <div>
                <strong>Карточка с рекомендацией</strong>
                <p>Используется для безопасного следующего шага.</p>
              </div>
              <button className="v2-button v2-button--primary">Выбрать</button>
            </article>
          </div>
        </KitSection>
        <DevKitExtensions />
      </div>
    </div>
  );
}

function KitSection({
  title,
  children,
  wide = false,
}: {
  title: string;
  children: React.ReactNode;
  wide?: boolean;
}) {
  return (
    <section className={`v2-kit-section ${wide ? "v2-kit-section--wide" : ""}`}>
      <h2>{title}</h2>
      <div className="v2-kit-section__body">{children}</div>
    </section>
  );
}

function StatusExample({
  label,
  tone,
}: {
  label: string;
  tone: "ready" | "active" | "muted";
}) {
  return <StatusChip tone={tone}>{label}</StatusChip>;
}

function MicIcon() {
  return (
    <svg
      className="v2-glow-outline-button__icon"
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      <rect x="8" y="3" width="8" height="12" rx="4" />
      <path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3M8.5 21h7" />
    </svg>
  );
}
