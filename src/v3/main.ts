import { createApp } from "vue";
import { WlConfig, WlToastService, wlLocaleRu } from "@whitelife-core/ui-kit";
import "@whitelife-core/ui-kit/styles/reset.css";
import "@whitelife-core/ui-kit/styles/base.css";
import "@whitelife-core/ui-kit/themes/graphite.css";
import "./styles.css";
import App from "./app/App.vue";
import { router } from "./app/router";
import { createMockWorkspace } from "./app/mockWorkspace";
import { workspaceKey } from "./shared/application/workspace";
import {
  createInteraction,
  interactionKey,
} from "./shared/application/interaction";
import { setupDocument } from "./shared/infrastructure/browser";
setupDocument();
const app = createApp(App);
app.use(WlConfig, { locale: wlLocaleRu });
app.use(WlToastService);
app.use(router);
app.provide(workspaceKey, createMockWorkspace());
app.provide(interactionKey, createInteraction());
app.mount("#app");
