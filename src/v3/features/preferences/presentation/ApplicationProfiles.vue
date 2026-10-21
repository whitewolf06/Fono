<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import type { ApplicationProfile } from "../../../shared/domain/applicationProfiles";
import { newApplicationProfile } from "../../../shared/domain/applicationProfiles";
import ApplicationProfileEditor from "./ApplicationProfileEditor.vue";
import { useWorkspace } from "../../../shared/application/workspace";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const editing = ref<ApplicationProfile | null>(null);
const others = computed(() =>
  draft.value.applicationProfiles.filter((p) => p.id !== editing.value?.id),
);
function store(profile: ApplicationProfile) {
  const index = draft.value.applicationProfiles.findIndex(
    (p) => p.id === profile.id,
  );
  const next = [...draft.value.applicationProfiles];
  if (index < 0) next.push(profile);
  else next[index] = profile;
  draft.value.applicationProfiles = next;
  editing.value = null;
}
function toggle(profile: ApplicationProfile) {
  draft.value.applicationProfiles = draft.value.applicationProfiles.map((p) =>
    p.id === profile.id ? { ...p, enabled: !p.enabled } : p,
  );
}
</script>
<template>
  <section id="application-profiles" class="form-stack">
    <div class="section-divider" />
    <h3>{{ $t("ui.add2110e07") }}</h3>
    <p class="muted">{{ $t("ui.ac501cd57c") }}</p>
    <p class="muted">{{ $t("ui.d421280611") }}</p>
    <p class="muted">{{ $t("ui.7865f62036") }}</p>
    <p v-if="!workspace.native" class="notice">{{ $t("ui.cfb19f4709") }}</p>
    <ul
      v-if="draft.applicationProfiles.length"
      class="dictionary-list"
      :aria-label="$t('ui.add2110e07')"
    >
      <li v-for="profile in draft.applicationProfiles" :key="profile.id">
        <div>
          <strong>{{ profile.name }}</strong
          ><small
            >{{ profile.executables.join(", ") }} ·
            {{
              profile.enabled ? $t("ui.0b81629905") : $t("ui.5a577d9056")
            }}</small
          >
        </div>
        <div class="actions">
          <WlButton
            size="sm"
            variant="ghost"
            :aria-label="$t('ui.a18e9485da') + profile.name"
            @click="editing = profile"
            >{{ $t("ui.b8b151d417") }}</WlButton
          ><WlButton
            size="sm"
            variant="ghost"
            :aria-label="$t('ui.820ceb0d28') + profile.name"
            @click="toggle(profile)"
            >{{
              profile.enabled ? $t("ui.eda262ba05") : $t("ui.a54eaf3785")
            }}</WlButton
          ><WlButton
            size="sm"
            variant="ghost"
            :aria-label="$t('ui.40531844bd') + profile.name"
            @click="
              draft.applicationProfiles = draft.applicationProfiles.filter(
                (p) => p.id !== profile.id,
              )
            "
            >{{ $t("ui.be99b13612") }}</WlButton
          >
        </div>
      </li>
    </ul>
    <p v-else class="muted">{{ $t("ui.37d1ae020b") }}</p>
    <div class="actions">
      <WlButton
        :disabled="draft.applicationProfiles.length >= 32"
        @click="editing = newApplicationProfile(draft.applicationProfiles)"
        >{{ $t("ui.55632da092") }}</WlButton
      ><small class="muted">{{ draft.applicationProfiles.length }} / 32</small>
    </div>
    <ApplicationProfileEditor
      v-if="editing"
      :profile="editing"
      :others="others"
      @close="editing = null"
      @save="store"
    />
  </section>
</template>
