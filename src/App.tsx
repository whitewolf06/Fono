import { useEffect, useState } from "react";
import type { AppView } from "./main";
import { SettingsView } from "./views/Settings";
import { OverlayView } from "./views/Overlay";
import { OnboardingView } from "./views/Onboarding";

export default function App({ view }: { view: AppView }) {
  // Overlay и Onboarding рендерятся в отдельные окна/режимы
  const [route, setRoute] = useState<AppView>(view);

  // Позволяем динамически переключать view (для дев-режима)
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const v = params.get("view") as AppView | null;
    if (v) setRoute(v);
  }, []);

  if (route === "overlay") {
    document.body.classList.add("overlay-mode");
    return <OverlayView />;
  }
  if (route === "onboarding") return <OnboardingView />;
  return <SettingsView />;
}
