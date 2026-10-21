<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed, ref, watch, nextTick } from "vue";
import {
  useRoute,
  useRouter,
  onBeforeRouteLeave,
  onBeforeRouteUpdate,
} from "vue-router";
import { WlInput, WlButton } from "@whitelife-core/ui-kit";
import type { Section } from "../../../shared/domain/contracts";
import { sectionKeys } from "../domain/preferences";
import { sections, searchIndex } from "../application/catalog";
import { useDraft } from "../application/useDraft";
import { useFeedback } from "../../../shared/application/feedback";
import { focusField } from "../../../shared/infrastructure/focus";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import AudioFields from "./AudioFields.vue";
import PretranscriptionField from "./PretranscriptionField.vue";
import ModelManager from "./ModelManager.vue";
import ActivationFields from "./ActivationFields.vue";
import ProcessingFields from "./ProcessingFields.vue";
import OverlayFields from "./OverlayFields.vue";
import PrivacyFields from "./PrivacyFields.vue";
import DiagnosticsPanel from "./DiagnosticsPanel.vue";
import DictionaryFields from "./DictionaryFields.vue";
import SnippetFields from "../../snippets/presentation/SnippetFields.vue";
import ApplicationProfiles from "./ApplicationProfiles.vue";
import { UpdatesPanel } from "../../updates";
import CudaPackagePanel from "../../cuda-packages/presentation/CudaPackagePanel.vue";
import { useWorkspace } from "../../../shared/application/workspace";
const workspace = useWorkspace();
const route = useRoute();
const router = useRouter();
const query = ref("");
const section = computed(() => (route.params.section as Section) || "general");
const current = computed(() => sections.find((s) => s.id === section.value)!);
const { draft, dirty, reset, save, canLeave } = useDraft(
  () => sectionKeys[section.value],
);
const { run, busy, error } = useFeedback();
const whisperSelected = computed(
  () =>
    (workspace.state.models.find((m) => m.id === draft.model)?.engine ||
      "whisper") === "whisper",
);
const version = __FONO_FRONTEND_BUILD__.version;
const results = computed(() =>
  searchIndex.filter((item) =>
    (item.label + " " + item.keywords)
      .toLocaleLowerCase("ru")
      .includes(query.value.trim().toLocaleLowerCase("ru")),
  ),
);
onBeforeRouteLeave(() => (busy.value ? false : canLeave()));
onBeforeRouteUpdate(async (to) => {
  if (busy.value) return false;
  if (to.params.section !== route.params.section && !(await canLeave()))
    return false;
});
watch(
  () => [route.params.section, route.query.field],
  async () => {
    query.value = "";
    await nextTick();
    if (typeof route.query.field === "string") focusField(route.query.field);
  },
  { immediate: true },
);
async function navigateResult(item: (typeof searchIndex)[number]) {
  await router.push({
    path: "/settings/" + item.section,
    query: { field: item.field },
  });
}
</script>
<template>
  <div class="page settings-page">
    <PageHeading
      :title="$t('ui.985b5e0f2c')"
      :description="$t('ui.fdc56f94a6')"
    />
    <div class="settings-search">
      <AppIcon name="search" /><WlInput
        v-model="query"
        :aria-label="$t('ui.50e5300abb')"
        :placeholder="$t('ui.92d9d59819')"
      />
    </div>
    <div v-if="query.trim()" class="settings-results" aria-live="polite">
      <button
        v-for="result in results"
        :key="result.label"
        @click="navigateResult(result)"
      >
        <span>{{ result.label }}</span
        ><small>{{
          sections.find((s) => s.id === result.section)?.title
        }}</small
        ><AppIcon name="chevron-right" />
      </button>
      <p v-if="!results.length">{{ $t("ui.05b11c0956") }}</p>
    </div>
    <div v-else class="settings-layout">
      <nav class="settings-nav" :aria-label="$t('ui.ab71d4cec4')">
        <RouterLink
          v-for="item in sections"
          :key="item.id"
          :to="'/settings/' + item.id"
          :class="{ selected: item.id === section }"
          :aria-current="item.id === section ? 'page' : undefined"
          ><AppIcon :name="item.icon" /><span>{{
            item.title
          }}</span></RouterLink
        >
      </nav>
      <section class="settings-content">
        <header>
          <h2>{{ current.title }}</h2>
          <p>{{ current.description }}</p>
        </header>
        <div v-if="section === 'general'" class="form-stack">
          <SelectField
            id="uiLanguage"
            v-model="draft.uiLanguage"
            :label="$t('ui.9089a73112')"
            :options="[
              {
                value: 'auto',
                get label() {
                  return $t('ui.29e4a4bd7a');
                },
              },
              {
                value: 'ru',
                get label() {
                  return $t('ui.6da140fdd4');
                },
              },
              { value: 'en', label: 'English' },
            ]"
          />
          <PreferenceToggle
            name="autostart"
            :label="$t('ui.b312f5a759')"
            :description="$t('ui.b587cc4370')"
          /><SelectField
            id="insertion"
            v-model="draft.insertion"
            :label="$t('ui.5e399aa003')"
            :options="[
              {
                value: 'clipboard',
                get label() {
                  return $t('ui.4e9b12d77d');
                },
              },
              {
                value: 'sendinput',
                get label() {
                  return $t('ui.f3cc63fc58');
                },
              },
            ]"
          />
          <ApplicationProfiles v-model="draft" />
          <section id="about" class="about-fono">
            <h3>Fono {{ version }}</h3>
            <p>{{ $t("ui.69f4caaaac") }}</p>
            <small
              >{{ $t("ui.edc3ee4f1c")
              }}{{ workspace.native ? "" : $t("ui.e3348b9eff") }}</small
            ><RouterLink class="text-link" to="/onboarding">{{
              $t("ui.b6964d42aa")
            }}</RouterLink>
          </section>
          <UpdatesPanel :unsaved="dirty" />
        </div>
        <template v-else-if="section === 'audio'"
          ><AudioFields
            v-model="draft" /><PretranscriptionField /><CudaPackagePanel
            v-if="whisperSelected"
            :unsaved="dirty" /><DictionaryFields
            v-model="draft" /><SnippetFields v-model="draft" /><ModelManager
        /></template>
        <ActivationFields
          v-else-if="section === 'activation'"
          v-model="draft"
          advanced
        />
        <ProcessingFields
          v-else-if="section === 'processing'"
          v-model="draft"
          advanced
        />
        <OverlayFields v-else-if="section === 'overlay'" v-model="draft" />
        <PrivacyFields v-else-if="section === 'privacy'" v-model="draft" />
        <DiagnosticsPanel v-else />
        <p v-if="error" class="error-text" role="alert">{{ error }}</p>
        <footer v-if="section !== 'diagnostics'" class="settings-save">
          <small>{{ dirty ? $t("ui.382dd64dc0") : $t("ui.10ab0c2bf1") }}</small
          ><WlButton
            :disabled="!dirty || busy || workspace.state.triggerCaptureActive"
            size="sm"
            @click="reset"
            >{{ $t("ui.8fbe9b75cb") }}</WlButton
          ><WlButton
            variant="primary"
            :disabled="!dirty || workspace.state.triggerCaptureActive"
            :loading="busy"
            size="sm"
            @click="run(save, $t('ui.babdb58139'))"
            >{{ $t("ui.b4d30cae52") }}</WlButton
          >
        </footer>
      </section>
    </div>
  </div>
</template>
