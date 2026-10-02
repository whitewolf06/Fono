<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { RouterLink, RouterView, useRouter } from "vue-router";
import {
  WlButton,
  WlDialog,
  WlToast,
  type WlIconName,
} from "@whitelife-core/ui-kit";
import { useWorkspace } from "../shared/application/workspace";
import { useInteraction } from "../shared/application/interaction";
import { bindShortcut } from "../shared/infrastructure/browser";
import AppIcon from "../shared/presentation/AppIcon.vue";
import FonoWordmark from "../shared/FonoWordmark.vue";
import CommandConfirmation from "../features/commands/presentation/CommandConfirmation.vue";
import PageLoadState from "../shared/presentation/PageLoadState.vue";
import QuickSettings from "../features/preferences/presentation/QuickSettings.vue";
const workspace = useWorkspace();
const router = useRouter();
const ui = useInteraction();
const version = __FONO_FRONTEND_BUILD__.version;
const menu: { to: string; title: string; icon: WlIconName | "server" }[] = [
  { to: "/", title: "Главная", icon: "home" },
  { to: "/history", title: "История", icon: "clock" },
  { to: "/trainer", title: "Речевой тренер", icon: "activity" },
  { to: "/commands", title: "Голосовые команды", icon: "microphone" },
  { to: "/api/tasks", title: "API-сервис", icon: "server" },
  { to: "/settings/general", title: "Настройки", icon: "settings" },
];
let releaseShortcut = () => {};
let releaseCommandsShortcut = () => {};
onMounted(() => {
  if (workspace.native) return;
  releaseCommandsShortcut = bindShortcut(
    () => workspace.state.preferences.commandHotkey,
    () => {
      if (!ui.state.quick && !ui.state.confirmation)
        void router.push("/commands");
    },
  );
  releaseShortcut = bindShortcut(
    () => workspace.state.preferences.hotkey,
    () => {
      if (ui.state.quick || ui.state.confirmation) return;
      if (["listening", "silence"].includes(workspace.state.phase))
        void workspace.dictation.finish();
      else workspace.dictation.start();
    },
  );
});
onUnmounted(() => {
  releaseShortcut();
  releaseCommandsShortcut();
  workspace.dispose();
});
</script>
<template>
  <div class="v3-shell">
    <aside class="v3-sidebar">
      <RouterLink class="v3-brand" to="/" aria-label="Fono — главная">
        <AppIcon name="activity" :size="30" /><span
          ><FonoWordmark class="brand-wordmark" /><small
            >Версия {{ version }}</small
          ></span
        >
      </RouterLink>
      <nav class="v3-side-nav" aria-label="Основная навигация">
        <RouterLink
          v-for="item in menu"
          :key="item.to"
          :to="item.to"
          class="nav-item"
          :class="{
            active:
              item.to === '/'
                ? $route.path === '/'
                : $route.path.startsWith(
                    item.to.split('/').slice(0, 2).join('/'),
                  ),
          }"
          :title="item.title"
          :aria-label="item.title"
          :aria-current="
            (
              item.to === '/'
                ? $route.path === '/'
                : $route.path.startsWith(
                    item.to.split('/').slice(0, 2).join('/'),
                  )
            )
              ? 'page'
              : undefined
          "
          ><AppIcon :name="item.icon" /><span>{{
            item.title
          }}</span></RouterLink
        >
      </nav>
      <div v-if="!workspace.native" class="sidebar-bottom">
        <span class="demo-label"><span />Демо интерфейса</span
        ><RouterLink
          to="/scenarios"
          class="nav-item"
          title="Проверка интерфейса"
          aria-label="Проверка интерфейса"
          ><AppIcon name="grid" /><span>Сценарии проверки</span></RouterLink
        >
      </div>
    </aside>
    <main id="main-content" class="v3-main">
      <div
        v-if="workspace.native && workspace.state.error"
        class="notice error"
        role="alert"
      >
        {{ workspace.state.error }}
        <WlButton size="xs" @click="workspace.state.error = ''"
          >Закрыть</WlButton
        >
      </div>
      <PageLoadState
        v-if="
          ['loading', 'load-error'].includes(workspace.state.scenario) &&
          $route.path !== '/scenarios'
        "
      /><RouterView v-else />
    </main>
  </div>
  <QuickSettings
    v-if="ui.state.quick"
    :key="ui.state.quick"
    :panel="ui.state.quick"
  />
  <WlDialog
    v-if="ui.state.confirmation"
    :visible="true"
    :pt="{ mask: { class: 'confirmation-mask' } }"
    :header="ui.state.confirmation?.title"
    width="440px"
    @update:visible="ui.answer(false)"
  >
    <p class="dialog-description">{{ ui.state.confirmation?.text }}</p>
    <template #footer
      ><WlButton @click="ui.answer(false)">{{
        ui.state.confirmation?.cancel || "Отмена"
      }}</WlButton
      ><WlButton
        :variant="ui.state.confirmation?.danger ? 'danger' : 'primary'"
        @click="ui.answer(true)"
        >{{ ui.state.confirmation?.accept }}</WlButton
      ></template
    >
  </WlDialog>
  <CommandConfirmation
    v-if="workspace.native && workspace.state.commandProposal"
  />
  <WlToast />
</template>
