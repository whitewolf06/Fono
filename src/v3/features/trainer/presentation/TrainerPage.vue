<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import { analyze } from "../domain/analysis";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { PreferenceToggle } from "../../preferences";
const workspace = useWorkspace();
const { busy, run, error } = useFeedback();
const period = ref(7);
const selectedId = ref("");
const recommendation = ref("");
const entries = computed(() =>
  workspace.state.history.filter(
    (e) =>
      e.original &&
      (!workspace.native || e.analysisReady) &&
      Date.now() - Date.parse(e.createdAt) <= period.value * 86400000,
  ),
);
const selected = computed(
  () =>
    entries.value.find((e) => e.id === selectedId.value) || entries.value[0],
);
const findings = computed(() =>
  selected.value ? analyze(selected.value) : [],
);
const totals = computed(() =>
  entries.value.reduce(
    (acc, entry) => {
      analyze(entry).forEach((item, i) => (acc[i] += item.count));
      return acc;
    },
    [0, 0, 0],
  ),
);
const trend = computed(() =>
  entries.value
    .slice()
    .reverse()
    .map((entry, i) => ({
      x: 28 + i * (430 / Math.max(1, entries.value.length - 1)),
      y:
        105 -
        Math.min(80, analyze(entry).reduce((n, f) => n + f.count, 0) * 23),
    })),
);
watch([selectedId, period], () => {
  recommendation.value = "";
});
async function recommend() {
  await run(async () => {
    if (workspace.trainer.recommend && selected.value) {
      recommendation.value = await workspace.trainer.recommend(
        selected.value.id,
      );
      return;
    }
    if (
      workspace.state.profiles.find(
        (p) => p.id === workspace.state.preferences.trainerProfile,
      )?.location === "cloud" &&
      !workspace.state.preferences.cloudConsent
    )
      throw new Error("Разрешите передачу данных в облако в настройках ИИ.");
    await workspace.settings.testConnection(
      workspace.state.preferences.trainerProfile,
    );
    recommendation.value =
      "Перед следующей диктовкой сформулируйте одну главную мысль. Говорите короткими фразами и делайте паузы вместо «ну». Затем сравните следующую запись с этой.";
  });
}
</script>
<template>
  <div class="page">
    <PageHeading
      title="Речевой тренер"
      description="Маленькие наблюдения для более ясной речи."
      ><PreferenceToggle name="trainerEnabled" label="Речевой тренер" compact
    /></PageHeading>
    <div
      v-if="!workspace.state.preferences.trainerEnabled"
      class="welcome-panel"
    >
      <div class="large-icon"><AppIcon name="activity" :size="36" /></div>
      <h2>Услышать себя по-новому</h2>
      <p>
        Fono замечает повторы и слова-паразиты в исходных расшифровках. Анализ
        остаётся на вашем компьютере.
      </p>
      <PreferenceToggle
        name="trainerEnabled"
        label="Включить речевого тренера"
        description="Потребуется разрешение сохранять исходные расшифровки."
      /><small v-if="!workspace.native"
        >Предпросмотр использует демонстрационные записи.</small
      >
    </div>
    <template v-else>
      <div class="section-header">
        <div class="segmented" aria-label="Период анализа">
          <button
            v-for="days in [7, 30, 90]"
            :key="days"
            :class="{ selected: period === days }"
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
            <small class="eyebrow">Ваша речь в динамике</small>
            <h2>{{ entries.length }} диктовки</h2>
            <p class="muted">Разобраны за выбранный период</p>
            <div
              v-for="(label, i) in [
                'Слова-паразиты',
                'Повторы',
                'Неуверенные обороты',
              ]"
              :key="label"
              class="metric-line"
            >
              <span>{{ label }}</span
              ><strong>{{ totals[i] }}</strong>
            </div>
          </div>
          <div class="trend-chart">
            <div class="section-header">
              <strong>Привычки на одну диктовку</strong
              ><small>меньше — лучше</small>
            </div>
            <svg
              viewBox="0 0 490 135"
              role="img"
              aria-label="Количество речевых привычек по диктовкам"
            >
              <path d="M20 30H470 M20 68H470 M20 106H470" class="chart-grid" />
              <polyline
                :points="trend.map((p) => p.x + ',' + p.y).join(' ')"
                class="chart-line"
              />
              <circle
                v-for="(point, i) in trend"
                :key="i"
                :cx="point.x"
                :cy="point.y"
                r="4"
              /></svg
            ><small class="muted">От ранних записей к последним</small>
          </div>
        </div>
        <div class="master-detail">
          <div class="entry-list">
            <h3 class="date-label">Разобранные диктовки</h3>
            <button
              v-for="entry in entries"
              :key="entry.id"
              class="entry-button"
              :class="{ selected: selected?.id === entry.id }"
              @click="selectedId = entry.id"
            >
              <strong>{{ entry.title }}</strong>
              <p>{{ entry.text }}</p>
              <small>{{
                new Date(entry.createdAt).toLocaleDateString("ru-RU")
              }}</small>
            </button>
          </div>
          <article v-if="selected" class="detail-pane">
            <h2>{{ selected.title }}</h2>
            <p class="reading-text compact">{{ selected.original }}</p>
            <div
              v-for="finding in findings"
              :key="finding.title"
              class="finding"
            >
              <div class="section-header">
                <strong>{{ finding.title }}</strong
                ><span class="count-badge">{{ finding.count }}</span>
              </div>
              <p v-if="finding.count">
                <em>{{ finding.example }}</em> — {{ finding.advice }}
              </p>
              <p v-else class="muted">В этой диктовке не обнаружены.</p>
            </div>
            <div class="recommendation">
              <AppIcon name="sparkle" />
              <div>
                <strong>Небольшая практика</strong>
                <p>
                  Запишите одну мысль за 30 секунд. Пауза между фразами поможет
                  избежать повторов.
                </p>
              </div>
            </div>
            <WlButton
              v-if="workspace.state.preferences.trainerAiEnabled"
              size="sm"
              :loading="busy"
              @click="recommend"
              >Получить рекомендацию ИИ</WlButton
            ><RouterLink
              v-else
              class="text-link"
              to="/settings/processing?field=trainerModel"
              >Настроить рекомендации через ИИ</RouterLink
            >
            <p v-if="recommendation" class="notice" role="status">
              {{ recommendation }}
              <small
                >{{
                  workspace.native ? "Ответ ИИ" : "Демонстрационный ответ"
                }}
                · {{ workspace.state.preferences.trainerModel }}</small
              >
            </p>
            <p v-if="error" role="alert" class="error-text">{{ error }}</p>
          </article>
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
