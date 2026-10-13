<script setup lang="ts">
import { WlBadge } from "@whitelife-core/ui-kit";
import type { TrainerAiContext } from "../domain/report";
import { formatMetric } from "../application/overview";

defineProps<{ context: TrainerAiContext; saved?: boolean }>();
const size = (bytes: number) => formatMetric(bytes / 1024);
</script>

<template>
  <div
    class="ai-context"
    :aria-label="saved ? 'Состав сохранённого запроса' : 'Состав запроса ИИ'"
  >
    <div class="context-heading">
      <strong>{{
        saved ? "Состав сохранённого запроса" : "Состав запроса"
      }}</strong>
      <WlBadge
        :value="context.reduced ? 'Часть данных сокращена' : 'Без сокращений'"
        :variant="context.reduced ? 'warn' : 'gray'"
      />
    </div>
    <dl class="context-counts">
      <div>
        <dt>Диктовок в метриках</dt>
        <dd>{{ context.metricsSessions }}</dd>
      </div>
      <div>
        <dt>Слов в метриках</dt>
        <dd>{{ formatMetric(context.metricsWords) }}</dd>
      </div>
      <div>
        <dt>Данные отчёта</dt>
        <dd>
          {{ size(context.payloadBytes) }} /
          {{ size(context.payloadByteLimit) }} КБ
        </dd>
      </div>
      <div>
        <dt>Паттернов</dt>
        <dd>{{ context.patternsShared }} из {{ context.patternsAvailable }}</dd>
      </div>
      <template v-if="context.dataScope === 'original_text'">
        <div>
          <dt>Исходников в запросе</dt>
          <dd>
            {{ context.sharedSessions }} из {{ context.availableSessions }}
          </dd>
        </div>
        <div>
          <dt>Слов в исходниках</dt>
          <dd>
            {{ formatMetric(context.sharedWords) }} из
            {{ formatMetric(context.availableWords) }}
          </dd>
        </div>
        <div>
          <dt>Сокращённых исходников</dt>
          <dd>{{ context.shortenedSessions }}</dd>
        </div>
      </template>
      <template v-if="context.dataScope !== 'metrics_only'">
        <div>
          <dt>Примеров</dt>
          <dd>
            {{ context.examplesShared }} из {{ context.examplesAvailable }}
          </dd>
        </div>
      </template>
      <div v-if="context.dataScope === 'findings'">
        <dt>Слов в примерах</dt>
        <dd>
          {{ formatMetric(context.sharedWords) }} из
          {{ formatMetric(context.availableWords) }}
        </dd>
      </div>
    </dl>
    <p v-if="context.omittedCharacters" class="context-omitted">
      {{
        context.dataScope === "original_text" ? "В исходниках" : "В примерах"
      }}
      не передаются {{ formatMetric(context.omittedWords) }} слов и
      {{ formatMetric(context.omittedCharacters) }} символов.
      <template v-if="context.omittedSessions"
        >Полностью пропущено диктовок: {{ context.omittedSessions }}.</template
      >
    </p>
    <p v-if="!saved" class="context-note">
      Лимит Fono — {{ size(context.payloadByteLimit) }} КБ данных отчёта. Размер
      контекстного окна выбранной модели неизвестен.
      <template v-if="context.dataScope === 'original_text'"
        >До {{ context.limits.maxSharedSessions }} исходников, до
        {{ formatMetric(context.limits.maxCharactersPerSession) }} символов
        каждый.</template
      >
      <template v-if="context.reduced"
        >Числовые метрики учитывают весь период.</template
      >
    </p>
  </div>
</template>

<style scoped>
.ai-context {
  display: grid;
  gap: var(--fono-space-3);
  min-width: 0;
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
  font-size: var(--fono-type-sm);
  line-height: 1.6;
}
.context-heading {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--fono-space-2);
}
.context-counts {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--fono-space-2) var(--fono-space-4);
  margin: 0;
}
.context-counts > div {
  display: grid;
  gap: var(--fono-space-1);
}
.context-counts dt,
.context-note {
  color: var(--fono-muted);
}
.context-counts dd {
  margin: 0;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  overflow-wrap: anywhere;
}
.context-omitted {
  color: var(--fono-warning);
}
.context-note,
.context-omitted {
  margin: 0;
  overflow-wrap: anywhere;
}
@media (max-width: 780px) {
  .context-counts {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
