<script setup lang="ts">
import { computed, ref } from "vue";
import { useWorkspace } from "../../../shared/application/workspace";
import {
  aggregateFindings,
  exactDateLabel,
  findingsCount,
  formatMetric,
  recordingDurationLabel,
  sortTrainerEntries,
  wordCount,
  type TrainerSort,
} from "../application/overview";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { PreferenceToggle } from "../../preferences";
import TrainerChart from "./TrainerChart.vue";
import TrainerDetails from "./TrainerDetails.vue";

const workspace = useWorkspace();
const period = ref(7);
const sort = ref<TrainerSort>("date");
const selectedId = ref("");
const entries = computed(() =>
  workspace.state.history.filter(
    (entry) =>
      entry.original &&
      (!workspace.native || entry.analysisReady) &&
      Date.now() - Date.parse(entry.createdAt) <= period.value * 86400000,
  ),
);
const sortedEntries = computed(() =>
  sortTrainerEntries(entries.value, sort.value),
);
const selected = computed(
  () =>
    entries.value.find((entry) => entry.id === selectedId.value) ||
    sortedEntries.value[0],
);
const totals = computed(() => aggregateFindings(entries.value));
const count = computed(() =>
  totals.value.reduce((sum, finding) => sum + finding.count, 0),
);
const words = computed(() =>
  entries.value.reduce((sum, entry) => sum + wordCount(entry), 0),
);
const density = computed(() =>
  words.value > 0 ? (count.value * 100) / words.value : 0,
);
</script>

<template>
  <div class="page">
    <PageHeading
      title="Речевой тренер"
      description="Замечайте привычки в речи и сравнивайте свои диктовки."
      ><PreferenceToggle name="trainerEnabled" label="Речевой тренер" compact
    /></PageHeading>
    <div
      v-if="!workspace.state.preferences.trainerEnabled"
      class="welcome-panel"
    >
      <div class="large-icon"><AppIcon name="activity" :size="36" /></div>
      <h2>Услышать себя по-новому</h2>
      <p>
        Fono замечает слова-паразиты, повторы и самопоправки в исходных
        расшифровках. Анализ остаётся на вашем компьютере.
      </p>
      <PreferenceToggle
        name="trainerEnabled"
        label="Включить речевого тренера"
        description="Потребуется разрешение сохранять исходные расшифровки."
      />
      <small v-if="!workspace.native"
        >Предпросмотр использует демонстрационные записи.</small
      >
    </div>
    <template v-else>
      <div class="section-header trainer-toolbar">
        <div class="segmented" aria-label="Период анализа">
          <button
            v-for="days in [7, 30, 90]"
            :key="days"
            :class="{ selected: period === days }"
            :aria-pressed="period === days"
            @click="period = days"
          >
            {{ days }} дней
          </button>
        </div>
        <RouterLink class="text-link" to="/settings/privacy"
          >Хранение данных</RouterLink
        >
      </div>
      <template v-if="entries.length">
        <div class="trainer-overview">
          <div class="trainer-stats">
            <small class="eyebrow">За выбранный период</small>
            <h2>
              <span class="trainer-summary-label">Диктовок:</span>
              {{ entries.length }}
            </h2>
            <p class="muted">Слов в исходных расшифровках: {{ words }}</p>
            <div class="trainer-summary">
              <div>
                <strong>{{ count }}</strong
                ><small>маркеров в речи</small>
              </div>
              <div>
                <strong>{{ formatMetric(density) }}</strong
                ><small>на 100 слов</small>
              </div>
            </div>
            <div
              v-for="finding in totals"
              :key="finding.title"
              class="metric-line"
            >
              <span>{{ finding.title }}</span
              ><strong>{{ finding.count }}</strong>
            </div>
          </div>
          <TrainerChart
            :entries="entries"
            :selected-id="selected?.id"
            @select="selectedId = $event"
          />
        </div>
        <div class="master-detail">
          <div class="entry-list">
            <div class="trainer-list-toolbar">
              <h3 class="date-label">Разобранные диктовки</h3>
              <SelectField
                v-model="sort"
                label="Сортировка"
                :options="[
                  { value: 'date', label: 'Сначала новые' },
                  { value: 'duration', label: 'Сначала долгие' },
                  { value: 'length', label: 'Сначала длинные тексты' },
                ]"
              />
            </div>
            <button
              v-for="entry in sortedEntries"
              :key="entry.id"
              class="entry-button trainer-entry"
              :class="{ selected: selected?.id === entry.id }"
              :aria-pressed="selected?.id === entry.id"
              @click="selectedId = entry.id"
            >
              <strong>{{ entry.title }}</strong>
              <p>{{ entry.original }}</p>
              <time :datetime="entry.createdAt">{{
                exactDateLabel(entry.createdAt)
              }}</time>
              <span class="trainer-entry-meta"
                ><span>{{ recordingDurationLabel(entry) }}</span
                ><span>Слов: {{ wordCount(entry) }}</span
                ><span>Маркеров: {{ findingsCount(entry) }}</span></span
              >
            </button>
          </div>
          <TrainerDetails v-if="selected" :entry="selected" />
        </div>
      </template>
      <EmptyState
        v-else
        title="Пока нечего разбирать"
        text="Новые диктовки с исходной расшифровкой появятся здесь. Сохранение истории и локальная аналитика должны быть включены."
        ><RouterLink to="/settings/privacy" class="text-link"
          >Проверить настройки хранения</RouterLink
        ></EmptyState
      >
    </template>
  </div>
</template>

<style scoped>
.trainer-toolbar {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.trainer-summary-label {
  font-size: var(--fono-type-lg);
  font-weight: normal;
  color: var(--fono-muted);
}
.trainer-summary {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-5);
  margin-bottom: var(--fono-space-4);
}
.trainer-summary > div {
  display: grid;
  gap: var(--fono-space-1);
}
.trainer-summary strong {
  color: var(--fono-electric-soft);
  font-size: var(--fono-type-xl);
  font-variant-numeric: tabular-nums;
}
.trainer-summary small {
  color: var(--fono-muted);
}
.trainer-list-toolbar {
  padding: var(--fono-space-3);
  border-bottom: 1px solid var(--fono-border);
}
.trainer-list-toolbar .date-label {
  padding: 0;
  margin-bottom: var(--fono-space-3);
}
.trainer-entry strong {
  overflow-wrap: anywhere;
}
.trainer-entry time {
  display: block;
  color: var(--fono-secondary);
  font-size: var(--fono-type-xs);
  font-variant-numeric: tabular-nums;
}
.trainer-entry-meta {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-1) var(--fono-space-3);
  margin-top: var(--fono-space-1);
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
}
</style>
