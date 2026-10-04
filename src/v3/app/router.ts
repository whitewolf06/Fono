import { createRouter, createWebHashHistory } from "vue-router";
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      component: () =>
        import("../features/dictation/presentation/HomePage.vue"),
    },
    {
      path: "/history",
      component: () =>
        import("../features/history/presentation/HistoryPage.vue"),
    },
    {
      path: "/releases",
      component: () =>
        import("../features/releases/presentation/ReleasesPage.vue"),
    },
    {
      path: "/updates",
      component: () =>
        import("../features/updates/presentation/UpdatesPage.vue"),
    },
    {
      path: "/trainer",
      component: () =>
        import("../features/trainer/presentation/TrainerPage.vue"),
    },
    {
      path: "/commands",
      component: () =>
        import("../features/commands/presentation/CommandsPage.vue"),
    },
    {
      path: "/api/:tab(tasks|connection)?",
      component: () =>
        import("../features/service/presentation/ServicePage.vue"),
    },
    {
      path: "/settings/:section(general|audio|activation|processing|overlay|privacy|diagnostics)?",
      component: () =>
        import("../features/preferences/presentation/SettingsPage.vue"),
    },
    {
      path: "/onboarding",
      component: () =>
        import("../features/onboarding/presentation/OnboardingPage.vue"),
    },
    {
      path: "/overlay",
      component: () =>
        import("../features/overlay/presentation/OverlayPage.vue"),
    },
    {
      path: "/overlay-sketch",
      component: () =>
        import("../features/overlay/presentation/OverlaySketchPage.vue"),
    },
    {
      path: "/scenarios",
      component: () =>
        import("../features/scenarios/presentation/ScenariosPage.vue"),
    },
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
  scrollBehavior: (_to, _from, saved) => saved ?? { top: 0 },
});
