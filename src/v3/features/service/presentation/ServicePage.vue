<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import { useRoute } from "vue-router";
import { WlButton, WlDialog } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import { PreferenceToggle } from "../../preferences";
import ConnectionPanel from "./ConnectionPanel.vue";
const route = useRoute();
const workspace = useWorkspace();
const { run } = useFeedback();
const selectedId = ref<string | null>(null);
const selected = computed(() =>
  workspace.state.jobs.find((j) => j.id === selectedId.value),
);
const visible = computed({
  get: () => !!selected.value,
  set: () => {
    selectedId.value = null;
  },
});
const jobs = computed(() => workspace.state.jobs);
const queue = computed(() =>
  jobs.value.filter((j) => ["queued", "running"].includes(j.state)),
);
const labels = {
  get queued() {
    return uiMessage("ui.b22a2454bd");
  },
  get running() {
    return uiMessage("ui.7894eb1ac6");
  },
  get done() {
    return uiMessage("ui.ef05d57959");
  },
  get error() {
    return uiMessage("ui.bf22dfeeba");
  },
  get cancelled() {
    return uiMessage("ui.86b78af7c2");
  },
};
</script>
<template>
  <div class="page">
    <PageHeading :title="$t('ui.1b580ef9f9')" :description="$t('ui.71048023d3')"
      ><PreferenceToggle
        name="serviceEnabled"
        :label="$t('ui.1b580ef9f9')"
        compact
    /></PageHeading>
    <div class="service-summary">
      <div>
        <StatusDot
          :health="workspace.state.preferences.serviceEnabled ? 'ready' : 'off'"
          :label="
            workspace.state.preferences.serviceEnabled
              ? $t('ui.d39515ca3e')
              : $t('ui.ed913eabb5')
          "
        /><code>{{ workspace.state.serviceAddress || "127.0.0.1:17832" }}</code>
      </div>
      <span>{{ queue.length }} {{ $t("ui.bc3fec6b2e") }}</span>
    </div>
    <p v-if="workspace.state.serviceError" class="notice error" role="alert">
      {{ workspace.state.serviceError }}
    </p>
    <nav class="page-tabs" :aria-label="$t('ui.de35470cc9')">
      <RouterLink
        to="/api/tasks"
        :class="{ selected: route.params.tab !== 'connection' }"
        >{{ $t("ui.03cc1c2d5a") }}</RouterLink
      ><RouterLink
        to="/api/connection"
        :class="{ selected: route.params.tab === 'connection' }"
        >{{ $t("ui.ab71afb97f") }}</RouterLink
      >
    </nav>
    <ConnectionPanel v-if="route.params.tab === 'connection'" />
    <template v-else>
      <div class="section-header">
        <h2>{{ $t("ui.667cbce2ef") }}</h2>
        <WlButton
          size="sm"
          v-if="!workspace.native"
          :disabled="!workspace.state.preferences.serviceEnabled"
          @click="run(() => workspace.service.enqueue(), $t('ui.d65faa460a'))"
          ><template #icon><AppIcon name="plus" /></template
          >{{ $t("ui.5b69a5a486") }}</WlButton
        >
      </div>
      <p v-if="!workspace.state.preferences.serviceEnabled" class="notice">
        {{ $t("ui.37c2713c15") }}
      </p>
      <p v-if="queue.length >= 4" class="notice">{{ $t("ui.0d5d5b3571") }}</p>
      <div v-if="jobs.length" class="job-list">
        <article v-for="job in jobs" :key="job.id" class="job-row">
          <span class="job-icon"
            ><AppIcon
              :name="
                job.state === 'error'
                  ? 'warn'
                  : job.state === 'done'
                    ? 'check'
                    : 'file'
              "
          /></span>
          <div class="job-copy">
            <button class="job-name" @click="selectedId = job.id">
              {{ job.name }}
            </button>
            <p v-if="job.error" class="error-text">{{ job.error }}</p>
            <small v-else
              >{{ job.id }} · {{ job.seconds }} {{ $t("ui.8dfc7d105b") }}</small
            >
          </div>
          <StatusDot
            :health="
              job.state === 'error'
                ? 'error'
                : job.state === 'done'
                  ? 'ready'
                  : job.state === 'cancelled'
                    ? 'off'
                    : 'loading'
            "
            :label="labels[job.state]"
          /><WlButton
            v-if="job.state === 'queued' || job.state === 'running'"
            size="sm"
            variant="ghost"
            @click="run(() => workspace.service.cancel(job.id))"
            >{{ $t("ui.7c47f7292b") }}</WlButton
          ><WlButton
            v-else-if="job.text"
            size="sm"
            variant="ghost"
            :aria-label="$t('ui.f289ceb080') + job.name"
            @click="
              run(() => workspace.copy(job.text || ''), $t('ui.f998a21644'))
            "
            ><template #icon><AppIcon name="copy" /></template></WlButton
          ><WlButton
            v-else-if="!workspace.native && job.state === 'error'"
            size="sm"
            variant="ghost"
            :disabled="!workspace.state.preferences.serviceEnabled"
            @click="run(() => workspace.service.retry(job.id))"
            >{{ $t("ui.5189135a61") }}</WlButton
          >
        </article>
      </div>
      <EmptyState
        v-else
        :title="$t('ui.fe0cc00877')"
        :text="workspace.native ? $t('ui.54a7b1001e') : $t('ui.206d04006f')"
      />
    </template>
    <WlDialog v-model:visible="visible" :header="selected?.name" width="560px"
      ><template v-if="selected"
        ><StatusDot
          :health="
            selected.state === 'error'
              ? 'error'
              : selected.state === 'done'
                ? 'ready'
                : 'loading'
          "
          :label="labels[selected.state]"
        />
        <p class="reading-text">
          {{ selected.text || selected.error || $t("ui.7f4649b2f3") }}
        </p></template
      ><template #footer
        ><WlButton
          v-if="selected?.text"
          @click="
            run(() => workspace.copy(selected?.text || ''), $t('ui.f998a21644'))
          "
          >{{ $t("ui.cd4665b319") }}</WlButton
        ><WlButton @click="selectedId = null">{{
          $t("ui.a7a4033657")
        }}</WlButton></template
      ></WlDialog
    >
  </div>
</template>
