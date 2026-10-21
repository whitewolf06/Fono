<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { ConnectionProfile } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import { useInteraction } from "../../../shared/application/interaction";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import ProfileEditor from "./ProfileEditor.vue";
const workspace = useWorkspace();
const ui = useInteraction();
const { run } = useFeedback();
const editing = ref<ConnectionProfile | null>(null);
async function remove(profile: ConnectionProfile) {
  if (
    await ui.confirm({
      get title() {
        return uiMessage("ui.b745f551a9");
      },
      text:
        uiMessage("ui.b1c56a4a65") +
        profile.name +
        (workspace.native
          ? uiMessage("ui.166ced38b1")
          : uiMessage("ui.24de842e85")),
      get accept() {
        return uiMessage("ui.be99b13612");
      },
      danger: true,
    })
  )
    await run(
      () => workspace.settings.removeProfile(profile.id),
      uiMessage("ui.925efb6367"),
    );
}
</script>
<template>
  <section class="profile-manager">
    <div class="section-header">
      <h3>{{ $t("ui.b0f5269bbb") }}</h3>
      <WlButton
        size="sm"
        variant="ghost"
        @click="
          editing = {
            id: '',
            name: '',
            provider: 'custom',
            location: 'local',
            url: 'http://127.0.0.1:1234/v1',
            model: '',
          }
        "
        ><template #icon><AppIcon name="plus" /></template
        >{{ $t("ui.71038c53bb") }}</WlButton
      >
    </div>
    <div
      v-for="profile in workspace.state.profiles"
      :key="profile.id"
      class="profile-row"
    >
      <div>
        <strong>{{ profile.name }}</strong>
        <p>{{ profile.url }}</p>
        <small
          >{{
            profile.location === "local"
              ? $t("ui.d5077a2034")
              : $t("ui.b09d21d043")
          }}
          · {{ profile.model }}</small
        >
      </div>
      <WlButton
        size="sm"
        variant="ghost"
        :aria-label="$t('ui.a18e9485da') + profile.name"
        @click="editing = { ...profile }"
        ><template #icon><AppIcon name="edit" /></template></WlButton
      ><WlButton
        size="sm"
        variant="danger-quiet"
        :disabled="
          workspace.state.preferences.profile === profile.id ||
          workspace.state.preferences.trainerProfile === profile.id
        "
        :title="
          workspace.state.preferences.profile === profile.id ||
          workspace.state.preferences.trainerProfile === profile.id
            ? $t('ui.536b4be9b8')
            : $t('ui.2dc5cc0353')
        "
        :aria-label="$t('ui.40531844bd') + profile.name"
        @click="remove(profile)"
        ><template #icon><AppIcon name="trash" /></template
      ></WlButton>
    </div>
    <ProfileEditor v-if="editing" :profile="editing" @close="editing = null" />
  </section>
</template>
