import { createApp } from "vue";
import PrimeVue from "primevue/config";
import { createWlPt, wlLocaleRu } from "@whitelife-core/ui-kit";
import App from "./app/App.vue";

import "@whitelife-core/ui-kit/styles/reset.css";
import "@whitelife-core/ui-kit/styles/base.css";
import "@whitelife-core/ui-kit/themes/white.css";
import "primeicons/primeicons.css";
import "./styles.css";

const app = createApp(App);
app.use(PrimeVue, { unstyled: true, pt: createWlPt(), locale: wlLocaleRu });
app.mount("#app");
