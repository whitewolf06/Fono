import { createApp } from "vue";
import { WlConfig, WlToastService, wlLocaleRu } from "@whitelife-core/ui-kit";
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
import { isDesktop } from "./shared/infrastructure/native/ipc";
import { workspaceKey } from "./shared/application/workspace";
import {
  createInteraction,
  interactionKey,
} from "./shared/application/interaction";
import { setupDocument } from "./shared/infrastructure/browser";
setupDocument();
const native = isDesktop();
const overlay = native && getCurrentWindow().label === "overlay";
const app = createApp(overlay ? NativeOverlay : App);
app.use(WlConfig, { locale: wlLocaleRu });
app.use(WlToastService);
app.use(router);
if (!overlay)
  app.provide(
    workspaceKey,
    native ? createNativeWorkspace() : createMockWorkspace(),
  );
router.beforeEach((to) => (native && to.path === "/scenarios" ? "/" : true));
app.provide(interactionKey, createInteraction());
app.mount("#app");
