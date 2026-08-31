import { useEffect, useMemo, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { ipc } from "@/lib/ipc";
import type { BuildInfo } from "@/lib/types";
import { CommandsPage } from "@/v2/features/commands/presentation/CommandsPage";
import { VoiceCommandConfirmationDialog } from "@/v2/features/commands/presentation/VoiceCommandConfirmationDialog";
import { createTauriCommandsDraftStore } from "@/v2/features/commands/infrastructure/tauriCommandsDraftStore";
import { VoiceStageLayout } from "@/v2/features/dictation/presentation/VoiceStageLayout";
import type { VoiceSetupTarget } from "@/v2/features/dictation/presentation/VoiceSetupCards";
import { createMockDictationRuntime } from "@/v2/features/dictation/infrastructure/mockDictationRuntime";
import { createTauriDictationRuntime } from "@/v2/features/dictation/infrastructure/tauriDictationRuntime";
import { createMockSpeechTrainerStore } from "@/v2/features/speech-trainer/infrastructure/mockSpeechTrainerStore";
import { createTauriSpeechTrainerStore } from "@/v2/features/speech-trainer/infrastructure/tauriSpeechTrainerStore";
import { SpeechTrainerPage } from "@/v2/features/speech-trainer/presentation/SpeechTrainerPage";
import { createMockServiceRuntime } from "@/v2/features/service/infrastructure/mockServiceRuntime";
import { createTauriServiceRuntime } from "@/v2/features/service/infrastructure/tauriServiceRuntime";
import { ServicePage } from "@/v2/features/service/presentation/ServicePage";
import { createTauriSettingsDraftStore } from "@/v2/features/settings/infrastructure/tauriSettingsDraftStore";
import { createMockLlmProfilesStore } from "@/v2/features/llm-settings/infrastructure/mockLlmProfilesStore";
import { createTauriLlmProfilesStore } from "@/v2/features/llm-settings/infrastructure/tauriLlmProfilesStore";
import { LlmProfilesPage } from "@/v2/features/llm-settings/presentation/LlmProfilesPage";
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
import { BuildInfoIndicator } from "@/v2/shared/presentation/components/BuildInfoIndicator";
import { UiKitPage } from "@/v2/shared/presentation/UiKitPage";
import "@/v2/shared/presentation/styles/index.css";

type NavigationItem =
  "voice" | "trainer" | "commands" | "service" | "llm" | "settings" | "kit";

const productNavigation: { id: NavigationItem; icon: string; label: string }[] =
  [
    { id: "voice", icon: "♩", label: "Голос" },
    { id: "trainer", icon: "◌", label: "Речевой тренер" },
    { id: "commands", icon: "⌘", label: "Команды" },
    { id: "service", icon: "⌁", label: "Сервис" },
    { id: "llm", icon: "✦", label: "AI" },
    { id: "settings", icon: "⚙", label: "Настройки" },
  ];

const isUiDevelopment = window.location.hostname === "localhost";
const isNativeRuntime = isTauri();

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
  const [backendBuildInfo, setBackendBuildInfo] = useState<BuildInfo | null>(
    null,
  );
  const [backendBuildError, setBackendBuildError] = useState(false);
  const runtime = useMemo(
    () =>
      isNativeRuntime
        ? createTauriDictationRuntime()
        : createMockDictationRuntime(),
    [],
  );
  const settingsStore = useMemo(
    () => (isNativeRuntime ? createTauriSettingsDraftStore() : undefined),
    [],
  );
  const serviceRuntime = useMemo(
    () =>
      isNativeRuntime
        ? createTauriServiceRuntime()
        : createMockServiceRuntime(),
    [],
  );
  const commandsStore = useMemo(
    () => (isNativeRuntime ? createTauriCommandsDraftStore() : undefined),
    [],
  );
  const speechTrainerStore = useMemo(
    () =>
      isNativeRuntime
        ? createTauriSpeechTrainerStore()
        : createMockSpeechTrainerStore(),
    [],
  );
  const llmProfilesStore = useMemo(
    () =>
      isNativeRuntime
        ? createTauriLlmProfilesStore()
        : createMockLlmProfilesStore(),
    [],
  );
  const isCleanVoicePage = activePage === "voice";

  useEffect(() => {
    if (!isNativeRuntime) return;

    let active = true;
    void ipc
      .getBuildInfo()
      .then((info) => {
        if (active) setBackendBuildInfo(info);
      })
      .catch(() => {
        if (active) setBackendBuildError(true);
      });

    return () => {
      active = false;
    };
  }, []);

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
            <BuildInfoIndicator
              backend={backendBuildInfo}
              backendError={backendBuildError}
              isNativeRuntime={isNativeRuntime}
            />
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
              {activePage === "commands" && (
                <CommandsPage store={commandsStore} />
              )}
              {activePage === "settings" && (
                <SettingsPage
                  focusSection={settingsSection}
                  store={settingsStore}
                />
              )}
              {activePage === "service" && (
                <ServicePage runtime={serviceRuntime} />
              )}
              {activePage === "trainer" && (
                <SpeechTrainerPage
                  store={speechTrainerStore}
                  onOpenPrivacySettings={() => {
                    setSettingsSection("privacy");
                    setActivePage("settings");
                  }}
                />
              )}
              {activePage === "llm" && (
                <LlmProfilesPage store={llmProfilesStore} />
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
          store={settingsStore}
        />
      )}
      {onboardingOpen && <OnboardingDialog onComplete={closeOnboarding} />}
      {isNativeRuntime && <VoiceCommandConfirmationDialog />}
    </div>
  );
}
