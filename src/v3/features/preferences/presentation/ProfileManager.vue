<script setup lang="ts">
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
      title: "Удалить профиль?",
      text: "Профиль «" + profile.name + "» будет удалён из демосессии.",
      accept: "Удалить",
      danger: true,
    })
  )
    await run(
      () => workspace.settings.removeProfile(profile.id),
      "Профиль удалён",
    );
}
</script>
<template>
  <section class="profile-manager">
    <div class="section-header">
      <h3>Профили подключений</h3>
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
            model: 'Qwen 3 · 8B',
          }
        "
        ><template #icon><AppIcon name="plus" /></template>Добавить</WlButton
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
          >{{ profile.location === "local" ? "На компьютере" : "В облаке" }} ·
          {{ profile.model }}</small
        >
      </div>
      <WlButton
        size="sm"
        variant="ghost"
        :aria-label="'Изменить профиль ' + profile.name"
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
            ? 'Сначала выберите другое подключение'
            : 'Удалить профиль'
        "
        :aria-label="'Удалить профиль ' + profile.name"
        @click="remove(profile)"
        ><template #icon><AppIcon name="trash" /></template
      ></WlButton>
    </div>
    <ProfileEditor v-if="editing" :profile="editing" @close="editing = null" />
  </section>
</template>
