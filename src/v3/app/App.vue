<script setup lang="ts">
import { message as uiMessage, message as $t } from "../shared/domain/messages";

import { onMounted, onUnmounted } from "vue";
import { RouterLink, RouterView, useRouter, useRoute } from "vue-router";
import { WlButton, WlDialog, WlToast } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../shared/application/workspace";
import { useInteraction } from "../shared/application/interaction";
import { bindShortcut } from "../shared/infrastructure/browser";
import AppIcon from "../shared/presentation/AppIcon.vue";
import FonoWordmark from "../shared/FonoWordmark.vue";
import CommandConfirmation from "../features/commands/presentation/CommandConfirmation.vue";
import SelectionPreview from "../features/selection/presentation/SelectionPreview.vue";
import PageLoadState from "../shared/presentation/PageLoadState.vue";
import QuickSettings from "../features/preferences/presentation/QuickSettings.vue";
import SidebarExtras from "./presentation/SidebarExtras.vue";
import type { AppIconName } from "../shared/domain/icons";
const workspace = useWorkspace();
const router = useRouter();
const route = useRoute();
const ui = useInteraction();
const version = __FONO_FRONTEND_BUILD__.version;
const menu: { to: string; title: string; icon: AppIconName }[] = [
  {
    to: "/",
    get title() {
      return uiMessage("ui.d328e53dc4");
    },
    icon: "home",
  },
  {
    to: "/files",
    get title() {
      return uiMessage("ui.9088ca9c0d");
    },
    icon: "audio-file",
  },
  {
    to: "/history",
    get title() {
      return uiMessage("ui.1417b7792d");
    },
    icon: "history",
  },
  {
    to: "/trainer",
    get title() {
      return uiMessage("ui.387054796e");
    },
    icon: "chart",
  },
  {
    to: "/commands",
    get title() {
      return uiMessage("ui.6c0adb50d0");
    },
    icon: "voice-command",
  },
  {
    to: "/api/tasks",
    get title() {
      return uiMessage("ui.1b580ef9f9");
    },
    icon: "terminal",
  },
  {
    to: "/settings/general",
    get title() {
      return uiMessage("ui.985b5e0f2c");
    },
    icon: "settings",
  },
];
const appIconUrl = import.meta.env.BASE_URL + "fono-icon.png";
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
    () =>
      workspace.state.preferences.dictationTrigger === "shortcut"
        ? workspace.state.preferences.hotkey
        : "",
    () => {
      if (ui.state.quick || ui.state.confirmation) return;
      if (
        workspace.state.preferences.dictationMode === "live" &&
        ["listening", "silence"].includes(workspace.state.phase)
      )
        void workspace.dictation.finish();
      else workspace.dictation.start();
    },
    () => {
      if (workspace.state.preferences.dictationMode === "standard")
        void workspace.dictation.finish();
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
      <RouterLink class="v3-brand" to="/" :aria-label="$t('ui.fa328ff65e')">
        <img
          class="brand-icon"
          :src="appIconUrl"
          width="32"
          height="32"
          alt=""
          aria-hidden="true"
          draggable="false"
        /><span
          ><FonoWordmark class="brand-wordmark" /><small
            >{{ $t("ui.c399489233") }} {{ version }}</small
          ></span
        >
      </RouterLink>
      <nav class="v3-side-nav" :aria-label="$t('ui.8fff0bba44')">
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
      <SidebarExtras />
      <div v-if="!workspace.native" class="sidebar-bottom">
        <span class="demo-label"><span />{{ $t("ui.de09f76386") }}</span
        ><RouterLink
          to="/scenarios"
          class="nav-item"
          :title="$t('ui.228609c7a8')"
          :aria-label="$t('ui.228609c7a8')"
          ><AppIcon name="grid" /><span>{{
            $t("ui.87a0a9c452")
          }}</span></RouterLink
        >
      </div>
    </aside>
    <main id="main-content" class="v3-main">
      <div
        v-if="workspace.native && workspace.updates.state.phase === 'available'"
        class="notice"
        role="status"
      >
        {{ $t("ui.a851eb3395") }} {{ workspace.updates.state.nextVersion }}
        <RouterLink class="text-link" to="/updates">{{
          $t("ui.603e460bf5")
        }}</RouterLink>
      </div>
      <div
        v-if="workspace.native && workspace.state.error && $route.path !== '/'"
        class="notice error"
        role="alert"
      >
        {{ workspace.state.error }}
        <WlButton size="xs" @click="workspace.state.error = ''">{{
          $t("ui.a7a4033657")
        }}</WlButton>
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
        ui.state.confirmation?.cancel || $t("ui.8fbe9b75cb")
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
  <SelectionPreview v-if="workspace.native || route.path !== '/overlay'" />
</template>
