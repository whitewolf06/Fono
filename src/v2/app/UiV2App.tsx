import { useMemo, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { CommandsPage } from "@/v2/features/commands/presentation/CommandsPage";
import { VoiceStageLayout } from "@/v2/features/dictation/presentation/VoiceStageLayout";
import type { VoiceSetupTarget } from "@/v2/features/dictation/presentation/VoiceSetupCards";
import { createMockDictationRuntime } from "@/v2/features/dictation/infrastructure/mockDictationRuntime";
import { createTauriDictationRuntime } from "@/v2/features/dictation/infrastructure/tauriDictationRuntime";
import {
  completeOnboarding,
  shouldShowOnboarding,
} from "@/v2/features/onboarding/infrastructure/onboardingPreferences";
import { OnboardingDialog } from "@/v2/features/onboarding/presentation/OnboardingDialog";
import type { SettingsSection } from "@/v2/features/settings/application/useSettingsDraft";
import {
  QuickSettingsDialog,
  type QuickSettingsTarget,
} from "@/v2/features/settings/presentation/QuickSettingsDialog";
import { SettingsPage } from "@/v2/features/settings/presentation/SettingsPage";
import { AppIcon } from "@/v2/shared/presentation/components/AppIcon";
import { UiKitPage } from "@/v2/shared/presentation/UiKitPage";
import "@/v2/shared/presentation/styles/index.css";

type NavigationItem = "voice" | "commands" | "settings" | "kit";

const productNavigation: { id: NavigationItem; icon: string; label: string }[] =
  [
    { id: "voice", icon: "♩", label: "Голос" },
    { id: "commands", icon: "⌘", label: "Команды" },
    { id: "settings", icon: "⚙", label: "Настройки" },
  ];

const isUiDevelopment = window.location.hostname === "localhost";

const navigation = isUiDevelopment
  ? [...productNavigation, { id: "kit" as const, icon: "◈", label: "UI kit" }]
  : productNavigation;

export function UiV2App() {
  const [activePage, setActivePage] = useState<NavigationItem>("voice");
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("general");
  const [quickSettingsTarget, setQuickSettingsTarget] =
    useState<QuickSettingsTarget | null>(null);
  const [onboardingOpen, setOnboardingOpen] = useState(shouldShowOnboarding);
  const runtime = useMemo(
    () =>
      isTauri() ? createTauriDictationRuntime() : createMockDictationRuntime(),
    [],
  );
  const isCleanVoicePage = activePage === "voice";

  const openSettings = (target: VoiceSetupTarget) => {
    setQuickSettingsTarget(target);
  };

  const closeOnboarding = () => {
    completeOnboarding();
    setOnboardingOpen(false);
  };

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
                onClick={() => {
                  if (item.id === "settings") setSettingsSection("general");
                  setActivePage(item.id);
                }}
              >
                <i>{item.icon}</i>
                {item.label}
              </button>
            ))}
          </nav>
          <div className="v2-sidebar__bottom">
            <button
              className="v2-sidebar-onboarding"
              type="button"
              onClick={() => setOnboardingOpen(true)}
            >
              Быстрый старт
            </button>
          </div>
        </aside>
        {isCleanVoicePage && (
          <main
            className="v2-main-content v2-voice-workspace"
            aria-label="Рабочая область голоса"
          >
            <VoiceStageLayout runtime={runtime} onOpenSettings={openSettings} />
          </main>
        )}
        {!isCleanVoicePage && (
          <main className="v2-main-content">
            <div className="v2-main-content__inner">
              {activePage === "commands" && <CommandsPage />}
              {activePage === "settings" && (
                <SettingsPage focusSection={settingsSection} />
              )}
              {activePage === "kit" && <UiKitPage />}
            </div>
          </main>
        )}
      </div>
      {quickSettingsTarget && (
        <QuickSettingsDialog
          target={quickSettingsTarget}
          onClose={() => setQuickSettingsTarget(null)}
        />
      )}
      {onboardingOpen && <OnboardingDialog onComplete={closeOnboarding} />}
    </div>
  );
}
