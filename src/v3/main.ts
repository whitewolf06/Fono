import { createApp } from "vue";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";
import {
  initializeLocalization,
  installMessages,
  whiteUiLocale,
  followUiLanguage,
  setUiLanguage,
  browserSystemLanguage,
} from "./shared/infrastructure/localization";
import type { UiLanguage } from "./shared/domain/locale";
import { readPreferences } from "./shared/infrastructure/browser";
import type { Settings } from "./shared/infrastructure/native/ipcTypes";
import "@whitelife-core/ui-kit/styles/reset.css";
import "@whitelife-core/ui-kit/styles/base.css";
import "@whitelife-core/ui-kit/themes/graphite.css";
import "./styles.css";
import App from "./app/App.vue";
import NativeOverlay from "./features/overlay/presentation/NativeOverlay.vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { router } from "./app/router";
import { createMockWorkspace } from "./app/mockWorkspace";
import { createNativeWorkspace } from "./app/nativeWorkspace";
import { isDesktop, call, subscribe } from "./shared/infrastructure/native/ipc";
import { waitForDesktopReady } from "./shared/infrastructure/native/startupReady";
import { workspaceKey } from "./shared/application/workspace";
import {
  createInteraction,
  interactionKey,
} from "./shared/application/interaction";
import {
  setupDocument,
  showStartupFailure,
} from "./shared/infrastructure/browser";
setupDocument();
const native = isDesktop();
initializeLocalization(
  native ? "auto" : readPreferences().uiLanguage,
  browserSystemLanguage(),
);
async function mount() {
  if (native)
    await waitForDesktopReady({
      listen: (ready) =>
        subscribe<boolean>("fono-runtime-ready", (value) => {
          if (value === true) ready();
        }),
      read: () => call<boolean>("get_runtime_ready"),
    });
  const overlay = native && getCurrentWindow().label === "overlay";
  if (native) {
    const locale = await call<{ language: UiLanguage; systemLanguage: string }>(
      "get_ui_locale",
    );
    initializeLocalization(locale.language, locale.systemLanguage);
  }
  const app = createApp(overlay ? NativeOverlay : App);
  installMessages(app);
  app.use(WlConfig, { locale: whiteUiLocale });
  app.use(WlToastService);
  app.use(router);
  if (!overlay) {
    const workspace = native ? createNativeWorkspace() : createMockWorkspace();
    followUiLanguage(() => workspace.state.preferences.uiLanguage);
    app.provide(workspaceKey, workspace);
  }
  if (native)
    await subscribe<Settings>("settings-changed", (settings) =>
      setUiLanguage(settings.ui_language ?? "auto"),
    );
  router.beforeEach((to) => (native && to.path === "/scenarios" ? "/" : true));
  app.provide(interactionKey, createInteraction());
  app.mount("#app");
}
void mount().catch(() => showStartupFailure());
