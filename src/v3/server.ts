import { createApp } from "vue";
import { WlConfig } from "@whitelife-core/ui-kit";
import "@whitelife-core/ui-kit/styles/reset.css";
import "@whitelife-core/ui-kit/styles/base.css";
import "@whitelife-core/ui-kit/themes/graphite.css";
import "./features/serverPanel/presentation/panel.css";
import ServerPanel from "./features/serverPanel/presentation/ServerPanel.vue";
import {
  createServerPanel,
  serverPanelKey,
} from "./features/serverPanel/application/panel";
import {
  createHttpPanel,
  preparePanelDocument,
} from "./features/serverPanel/infrastructure/httpPanel";

const panel = createServerPanel(createHttpPanel());
preparePanelDocument(() => panel.logout());
const app = createApp(ServerPanel);
app.use(WlConfig);
app.provide(serverPanelKey, panel);
app.mount("#app");
