<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Dictation } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { analyze } from "../domain/analysis";
import {
  exactDateLabel,
  findingsCount,
  formatMetric,
  recordingDurationLabel,
  wordCount,
} from "../application/overview";

const props = defineProps<{ entry: Dictation }>();
const workspace = useWorkspace();
const { busy, run, error } = useFeedback();
const recommendation = ref("");
const findings = computed(() => analyze(props.entry));
const density = computed(() =>
  wordCount(props.entry) > 0
    ? (findingsCount(props.entry) * 100) / wordCount(props.entry)
    : 0,
);
const technicalFacts = computed(() => {
  const metadata = props.entry.metadata;
  const facts: string[] = [];
  if (metadata?.backend)
    facts.push(
      { cpu: "CPU", cuda: "NVIDIA CUDA", vulkan: "Vulkan" }[metadata.backend],
    );
  if (metadata?.model) facts.push(metadata.model);
  if (
    metadata?.generationDurationMs != null &&
    Number.isFinite(metadata.generationDurationMs) &&
    metadata.generationDurationMs >= 0
  )
    facts.push(
      `Генерация ${formatMetric(metadata.generationDurationMs / 1000)} с`,
    );
  return facts.join(" · ");
});
watch(
  () => props.entry.id,
  () => {
    recommendation.value = "";
    error.value = "";
  },
);
async function recommend() {
  const entryId = props.entry.id;
  await run(async () => {
    let answer: string;
    if (workspace.trainer.recommend)
      answer = await workspace.trainer.recommend(entryId);
    else {
      if (workspace.native)
        throw new Error(
          "Рекомендации ИИ сейчас недоступны. Проверьте подключение в настройках обработки текста.",
        );
      if (
        workspace.state.profiles.find(
          (profile) =>
            profile.id === workspace.state.preferences.trainerProfile,
        )?.location === "cloud" &&
        !workspace.state.preferences.cloudConsent
      )
        throw new Error("Разрешите передачу данных в облако в настройках ИИ.");
      await workspace.settings.testConnection(
        workspace.state.preferences.trainerProfile,
      );
      answer =
        "Перед следующей диктовкой сформулируйте одну главную мысль. Говорите короткими фразами и делайте паузы вместо «ну». Затем сравните следующую запись с этой.";
    }
    if (props.entry.id === entryId) recommendation.value = answer;
  });
}
</script>

<template>
  <article class="detail-pane trainer-detail">
    <div class="trainer-detail-heading">
      <h2>{{ entry.title }}</h2>
      <time :datetime="entry.createdAt" :title="entry.createdAt">{{
        exactDateLabel(entry.createdAt)
      }}</time>
    </div>
    <div class="trainer-detail-facts">
      <span
        ><AppIcon name="clock" :size="14" />{{
          recordingDurationLabel(entry)
        }}</span
      >
      <span>Слов в расшифровке: {{ wordCount(entry) }}</span>
      <span
        >Маркеров: {{ findingsCount(entry) }} · {{ formatMetric(density) }} на
        100 слов</span
      >
    </div>
    <p v-if="technicalFacts" class="muted trainer-technical">
      {{ technicalFacts }}
    </p>
    <div class="section-header trainer-original-heading">
      <small class="eyebrow">Исходная расшифровка</small>
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="busy"
        @click="
          run(
            () => workspace.copy(entry.original || ''),
            'Расшифровка скопирована',
          )
        "
        ><template #icon><AppIcon name="copy" :size="14" /></template
        >Копировать</WlButton
      >
    </div>
    <div class="reading-text compact" tabindex="0">{{ entry.original }}</div>
    <p v-if="!findingsCount(entry)" class="notice trainer-clean" role="status">
      Речевые маркеры не найдены. Можно сравнить эту диктовку со следующими.
    </p>
    <div v-for="finding in findings" :key="finding.title" class="finding">
      <div class="section-header">
        <strong>{{ finding.title }}</strong
        ><span class="count-badge">{{ finding.count }}</span>
      </div>
      <p v-if="finding.count">
        <em>{{ finding.example || "Обнаружены в исходной расшифровке" }}</em> —
        {{ finding.advice }}
      </p>
      <p v-else class="muted">В этой диктовке не обнаружены.</p>
    </div>
    <small class="muted"
      >Это наблюдения по тексту, а не оценка правильности речи. Контекст может
      менять значение маркера.</small
    >
    <div class="recommendation">
      <AppIcon name="sparkle" />
      <div>
        <strong>Небольшая практика</strong>
        <p>
          Сформулируйте одну мысль за 30 секунд. Пауза между фразами поможет
          избежать повторов.
        </p>
      </div>
    </div>
    <WlButton
      v-if="workspace.state.preferences.trainerAiEnabled"
      size="sm"
      :loading="busy"
      :disabled="busy"
      @click="recommend"
      >Получить рекомендацию ИИ</WlButton
    >
    <RouterLink
      v-else
      class="text-link"
      to="/settings/processing?field=trainerModel"
      >Настроить рекомендации через ИИ</RouterLink
    >
    <p v-if="recommendation" class="notice" role="status">
      {{ recommendation
      }}<small
        >{{ workspace.native ? "Ответ ИИ" : "Демонстрационный ответ" }} ·
        {{ workspace.state.preferences.trainerModel }}</small
      >
    </p>
    <p v-if="error" role="alert" class="error-text">{{ error }}</p>
  </article>
</template>

<style scoped>
.trainer-detail {
  min-width: 0;
}
.trainer-detail-heading {
  display: grid;
  gap: var(--fono-space-2);
  margin-bottom: var(--fono-space-3);
}
.trainer-detail-heading h2 {
  overflow-wrap: anywhere;
}
.trainer-detail-heading time {
  color: var(--fono-secondary);
  font-size: var(--fono-type-sm);
  font-variant-numeric: tabular-nums;
}
.trainer-detail-facts {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-2) var(--fono-space-3);
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
}
.trainer-detail-facts span {
  display: inline-flex;
  align-items: center;
  gap: var(--fono-space-1);
}
.trainer-technical {
  font-size: var(--fono-type-xs);
  margin-top: var(--fono-space-2);
  overflow-wrap: anywhere;
}
.trainer-original-heading {
  margin-top: var(--fono-space-4);
  flex-wrap: wrap;
  gap: var(--fono-space-2);
}
.trainer-clean {
  margin-block: var(--fono-space-3);
}
</style>
