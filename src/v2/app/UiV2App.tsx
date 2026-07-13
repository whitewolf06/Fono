import { useMemo, useState } from "react";
import { VoiceDashboard } from "@/v2/features/dictation/presentation/VoiceDashboard";
import { VoiceStageLayout } from "@/v2/features/dictation/presentation/VoiceStageLayout";
import { createMockDictationRuntime } from "@/v2/features/dictation/infrastructure/mockDictationRuntime";
import { AppIcon } from "@/v2/shared/presentation/components/AppIcon";
import { UiKitPage } from "@/v2/shared/presentation/UiKitPage";
import "@/v2/shared/presentation/styles/index.css";

type NavigationItem = "voice" | "voice-copy" | "commands" | "settings" | "kit";

const productNavigation: { id: NavigationItem; icon: string; label: string }[] =
  [
    { id: "voice", icon: "♩", label: "Голос" },
    { id: "voice-copy", icon: "◌", label: "Голос · копия" },
    { id: "commands", icon: "⌘", label: "Команды" },
    { id: "settings", icon: "⚙", label: "Настройки" },
  ];

const isUiDevelopment = window.location.hostname === "localhost";

const navigation = isUiDevelopment
  ? [...productNavigation, { id: "kit" as const, icon: "◈", label: "UI kit" }]
  : productNavigation;

export function UiV2App() {
  const [activePage, setActivePage] = useState<NavigationItem>("voice");
  const runtime = useMemo(() => createMockDictationRuntime(), []);
  const isCleanVoicePage = activePage === "voice";

  return (
    <div className="v2-root">
      <div
        className={`v2-app-shell ${isCleanVoicePage ? "v2-app-shell--voice" : ""}`}
      >
        <aside className="v2-sidebar">
          <div className="v2-brand">
            <AppIcon />
            <div>
              <strong>Fono</strong>
              <span>Локальная диктовка</span>
            </div>
          </div>
          <nav className="v2-navigation" aria-label="Основная навигация">
            {navigation.map((item) => (
              <button
                key={item.id}
                type="button"
                className={item.id === activePage ? "is-active" : ""}
                onClick={() => setActivePage(item.id)}
              >
                <i>{item.icon}</i>
                {item.label}
              </button>
            ))}
          </nav>
        </aside>
        {isCleanVoicePage && (
          <main
            className="v2-main-content v2-voice-workspace"
            aria-label="Рабочая область голоса"
          >
            <VoiceStageLayout runtime={runtime} />
          </main>
        )}
        {!isCleanVoicePage && (
          <main className="v2-main-content">
            {activePage === "voice-copy" && (
              <VoiceDashboard runtime={runtime} />
            )}
            {activePage === "commands" && (
              <PlannedPage
                title="Команды"
                description="Экран будет подключён после утверждения состава и поведения голосовых команд."
              />
            )}
            {activePage === "settings" && (
              <PlannedPage
                title="Настройки"
                description="Структура категорий уже зафиксирована в документе UI v2; следующий экран будет собран на общем UI-kit."
              />
            )}
            {activePage === "kit" && <UiKitPage />}
          </main>
        )}
        {!isCleanVoicePage && (
          <aside className="v2-context-panel">
            <section className="v2-glass-card">
              <h2>Сейчас</h2>
              <p>UI запущен на моках. Нативная часть Fono не требуется.</p>
              <span className="v2-context-tag">Vite UI mode</span>
            </section>
            <section className="v2-glass-card">
              <h2>Быстрые действия</h2>
              <button type="button" onClick={() => setActivePage("settings")}>
                Открыть настройки <span>→</span>
              </button>
              <button type="button" onClick={() => setActivePage("commands")}>
                Посмотреть команды <span>→</span>
              </button>
            </section>
          </aside>
        )}
      </div>
    </div>
  );
}

function PlannedPage({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <section className="v2-planned-page">
      <p className="v2-kicker">UI v2</p>
      <h1>{title}</h1>
      <p>{description}</p>
      <span>Не подключено к runtime намеренно.</span>
    </section>
  );
}
