<script setup lang="ts">
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
import ModelManager from "./ModelManager.vue";
import ActivationFields from "./ActivationFields.vue";
import ProcessingFields from "./ProcessingFields.vue";
import OverlayFields from "./OverlayFields.vue";
import PrivacyFields from "./PrivacyFields.vue";
import DiagnosticsPanel from "./DiagnosticsPanel.vue";
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
      title="Настройки"
      description="Частое — под рукой. Остальное — на своём месте."
    />
    <div class="settings-search">
      <AppIcon name="search" /><WlInput
        v-model="query"
        aria-label="Поиск настроек"
        placeholder="Поиск: микрофон, тишина, модель…"
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
      <p v-if="!results.length">
        Ничего не найдено. Попробуйте «звук», «ИИ» или «история».
      </p>
    </div>
    <div v-else class="settings-layout">
      <nav class="settings-nav" aria-label="Раздел настроек">
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
          <PreferenceToggle
            name="autostart"
            label="Запускать вместе с Windows"
            description="Fono будет готов к диктовке после входа в систему."
          /><SelectField
            id="insertion"
            v-model="draft.insertion"
            label="Способ вставки текста"
            :options="[
              {
                value: 'clipboard',
                label: 'Через буфер обмена · рекомендуется',
              },
              { value: 'sendinput', label: 'Эмуляция нажатий клавиш' },
            ]"
          />
          <section id="about" class="about-fono">
            <h3>Fono {{ version }}</h3>
            <p>Голосовой ввод и управление компьютером.</p>
            <small
              >Интерфейс V3 · WhiteUI 0.6.0{{
                workspace.native ? "" : " · демонстрационный режим"
              }}</small
            ><RouterLink class="text-link" to="/onboarding"
              >Пройти первоначальную настройку</RouterLink
            >
          </section>
        </div>
        <template v-else-if="section === 'audio'"
          ><AudioFields v-model="draft" /><ModelManager
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
          <small>{{
            dirty
              ? "Есть несохранённые изменения"
              : "Выключатели применяются сразу"
          }}</small
          ><WlButton :disabled="!dirty || busy" size="sm" @click="reset"
            >Отмена</WlButton
          ><WlButton
            variant="primary"
            :disabled="!dirty"
            :loading="busy"
            size="sm"
            @click="run(save, 'Настройки сохранены')"
            >Сохранить</WlButton
          >
        </footer>
      </section>
    </div>
  </div>
</template>
