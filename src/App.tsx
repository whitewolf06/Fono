import { useEffect, useState } from "react";
import type { AppView } from "./main";
import { SettingsView } from "./views/Settings";
import { OverlayView } from "./views/Overlay";
import { OnboardingView } from "./views/Onboarding";
import { UiV2App } from "./v2/app/UiV2App";
import { OverlayV2 } from "./v2/features/overlay/presentation/OverlayV2";

export default function App({ view }: { view: AppView }) {
  // Overlay и Onboarding рендерятся в отдельные окна/режимы
  const [route, setRoute] = useState<AppView>(view);

  // Позволяем динамически переключать view (для дев-режима)
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const v = params.get("view") as AppView | null;
    if (v) setRoute(v);
  }, []);

  const isUiV2 = new URLSearchParams(window.location.search).get("ui") === "v2";

  if (isUiV2 && route === "overlay") return <OverlayV2 />;
  if (isUiV2) return <UiV2App />;

  if (route === "overlay") {
    document.body.classList.add("overlay-mode");
    return <OverlayView />;
  }
  if (route === "onboarding") return <OnboardingView />;
  return <SettingsView />;
}
