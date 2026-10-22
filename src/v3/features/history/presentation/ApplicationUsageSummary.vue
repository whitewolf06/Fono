<script setup lang="ts">
import { computed } from "vue";
import type { Dictation } from "../../../shared/domain/contracts";
import { message as $t } from "../../../shared/domain/messages";
import { applicationUsage } from "../application/applicationUsage";
import { uiIntlLocale } from "../../../shared/domain/locale";
const props = defineProps<{ entries: Dictation[]; demo: boolean }>();
const summary = computed(() => applicationUsage(props.entries));
const share = (value: number) =>
  new Intl.NumberFormat(uiIntlLocale(), {
    style: "percent",
    maximumFractionDigits: 1,
  }).format(value);
</script>
<template>
  <details v-if="summary.total" class="application-usage">
    <summary>
      {{ $t("applicationContext.summary") }}: {{ summary.total }}
    </summary>
    <p class="muted">{{ $t("applicationContext.summaryNote") }}</p>
    <p v-if="demo" class="notice">{{ $t("applicationContext.demo") }}</p>
    <table>
      <caption class="muted">
        {{
          $t("applicationContext.period")
        }}
      </caption>
      <thead>
        <tr>
          <th scope="col">{{ $t("applicationContext.title") }}</th>
          <th scope="col">{{ $t("applicationContext.attempts") }}</th>
          <th scope="col">{{ $t("applicationContext.share") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in summary.rows" :key="row.id">
          <th scope="row">{{ row.name }}</th>
          <td>{{ row.attempts }}</td>
          <td>{{ share(row.share) }}</td>
        </tr>
      </tbody>
    </table>
  </details>
</template>
<style scoped>
.application-usage {
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border-soft);
  border-radius: var(--fono-radius);
  min-width: 0;
}
.application-usage summary {
  cursor: pointer;
  font-weight: 600;
}
.application-usage table {
  width: 100%;
  table-layout: fixed;
  border-collapse: collapse;
  font-size: var(--fono-type-sm);
}
.application-usage th,
.application-usage td {
  text-align: left;
  padding: var(--fono-space-2);
  overflow-wrap: anywhere;
}
.application-usage caption {
  text-align: left;
  padding-block-end: var(--fono-space-2);
}
</style>
