<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, watch } from "vue";
import { WlInput, WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import { filterHistory, groupHistory, timeLabel } from "../application/history";
import { exactDateLabel, recordingLabel } from "../application/statistics";
import HistoryStatistics from "./HistoryStatistics.vue";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const ui = useInteraction();
const { run, busy, error } = useFeedback();
const query = ref("");
const period = ref(0);
const selectedId = ref(workspace.state.history[0]?.id);
const variant = ref<"original" | "result">("result");
const filtered = computed(() =>
  filterHistory(workspace.state.history, query.value, period.value),
);
const groups = computed(() => groupHistory(filtered.value));
const selected = computed(() =>
  filtered.value.find((e) => e.id === selectedId.value),
);
watch(filtered, (entries) => {
  if (!entries.some((e) => e.id === selectedId.value))
    selectedId.value = entries[0]?.id;
});
watch(selectedId, () => {
  variant.value = "result";
  error.value = "";
});
const text = computed(() =>
  variant.value === "original"
    ? (selected.value?.original ?? selected.value?.text)
    : selected.value?.text,
);
async function remove() {
  if (
    selected.value &&
    (await ui.confirm({
      get title() {
        return uiMessage("ui.9d5ff42384");
      },
      get text() {
        return uiMessage("ui.a9b253362f");
      },
      get accept() {
        return uiMessage("ui.be99b13612");
      },
      danger: true,
    }))
  )
    await run(() => workspace.history.remove(selected.value!.id));
}
async function clear() {
  if (
    await ui.confirm({
      get title() {
        return uiMessage("ui.39bb078706");
      },
      get text() {
        return uiMessage("ui.f7fe51e83b");
      },
      get accept() {
        return uiMessage("ui.3f70f0ad67");
      },
      danger: true,
    })
  )
    await run(() => workspace.history.clear());
}
async function toggleIncluded() {
  const entry = selected.value;
  if (!entry || busy.value) return;
  const included = entry.analyticsIncluded === false;
  await run(
    () => workspace.history.setIncluded(entry.id, included),
    included ? uiMessage("ui.e99020539a") : uiMessage("ui.0409cba3fd"),
  );
}
</script>
<template>
  <div class="page">
    <PageHeading :title="$t('ui.1417b7792d')" :description="$t('ui.600002f23d')"
      ><WlButton
        size="sm"
        variant="danger-quiet"
        :disabled="busy || !workspace.state.history.length"
        @click="clear"
        ><template #icon><AppIcon name="trash" :size="16" /></template
        >{{ $t("ui.8965271d3c") }}</WlButton
      ></PageHeading
    >
    <p v-if="!workspace.state.preferences.historyEnabled" class="notice">
      {{ $t("ui.e8731f62da") }}
      <RouterLink to="/settings/privacy">{{ $t("ui.d71875872c") }}</RouterLink>
    </p>
    <div class="search-toolbar">
      <div class="search-input">
        <AppIcon name="search" /><WlInput
          v-model="query"
          :aria-label="$t('ui.2dc9e0798c')"
          :placeholder="$t('ui.019b9c3460')"
        />
      </div>
      <SelectField
        v-model="period"
        :label="$t('ui.e97ca72e2c')"
        :options="[
          {
            value: 0,
            get label() {
              return $t('ui.126e978f06');
            },
          },
          {
            value: 1,
            get label() {
              return $t('ui.5bb7883e25');
            },
          },
          {
            value: 7,
            get label() {
              return $t('ui.7867a2095f');
            },
          },
          {
            value: 30,
            get label() {
              return $t('ui.254d55672b');
            },
          },
        ]"
      />
    </div>
    <div v-if="filtered.length" class="master-detail">
      <div class="entry-list">
        <section v-for="group in groups" :key="group.date">
          <h3 class="date-label">{{ group.date }}</h3>
          <button
            v-for="entry in group.entries"
            :key="entry.id"
            class="entry-button"
            :class="{ selected: selectedId === entry.id }"
            :aria-pressed="selectedId === entry.id"
            :disabled="busy"
            @click="selectedId = entry.id"
          >
            <span class="entry-title">{{ entry.title }}</span>
            <p>{{ entry.text }}</p>
            <small
              >{{ timeLabel(entry.createdAt) }} ·
              {{ recordingLabel(entry) }}</small
            >
            <small
              v-if="entry.analyticsIncluded === false"
              class="history-excluded"
              >{{ $t("ui.084c75ac8f") }}</small
            >
          </button>
        </section>
      </div>
      <article v-if="selected" class="detail-pane">
        <div class="section-header history-detail-heading">
          <h2>{{ selected.title }}</h2>
          <time :datetime="selected.createdAt" :title="selected.createdAt">{{
            exactDateLabel(selected.createdAt)
          }}</time>
        </div>
        <HistoryStatistics :entry="selected" />
        <div class="segmented small">
          <button
            v-if="selected.original"
            :class="{ selected: variant === 'original' }"
            @click="variant = 'original'"
          >
            {{ $t("ui.421252fcb5") }}</button
          ><button
            :class="{ selected: variant === 'result' }"
            @click="variant = 'result'"
          >
            {{ $t("ui.4997a5143d") }}
          </button>
        </div>
        <p v-if="!selected.original" class="muted">{{ $t("ui.227ae563c2") }}</p>
        <div class="reading-text" tabindex="0">{{ text }}</div>
        <p class="muted history-analytics-state">
          {{
            selected.analyticsIncluded === false
              ? $t("ui.5135baf5a3")
              : $t("ui.fc61757a40")
          }}
        </p>
        <div class="actions">
          <WlButton
            size="sm"
            :disabled="busy"
            @click="run(() => workspace.copy(text || ''), $t('ui.ec655e25c5'))"
            ><template #icon><AppIcon name="copy" /></template
            >{{ $t("ui.1ecddf40bf") }}</WlButton
          ><WlButton
            size="sm"
            variant="ghost"
            :disabled="busy"
            @click="toggleIncluded"
            >{{
              selected.analyticsIncluded === false
                ? $t("ui.3234cf8f16")
                : $t("ui.dc7e313c2e")
            }}</WlButton
          ><WlButton
            size="sm"
            variant="danger-quiet"
            :disabled="busy"
            @click="remove"
            >{{ $t("ui.5f3a315499") }}</WlButton
          >
        </div>
        <p v-if="error" class="error-text" role="alert">{{ error }}</p>
      </article>
    </div>
    <EmptyState
      v-else
      :title="
        workspace.state.history.length
          ? $t('ui.480fae1c4b')
          : $t('ui.6e15137527')
      "
      :text="
        workspace.state.history.length
          ? $t('ui.fdba4c962f')
          : $t('ui.51ff2c20d6')
      "
      ><WlButton
        v-if="query || period"
        size="sm"
        @click="
          query = '';
          period = 0;
        "
        >{{ $t("ui.cd45ec7836") }}</WlButton
      ><RouterLink v-else class="text-link" to="/">{{
        $t("ui.ad94907f64")
      }}</RouterLink></EmptyState
    >
  </div>
</template>

<style scoped>
.history-detail-heading {
  align-items: flex-start;
  flex-wrap: wrap;
}
.history-detail-heading h2 {
  min-width: 0;
  overflow-wrap: anywhere;
}
.history-detail-heading time {
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
  font-variant-numeric: tabular-nums;
}
.history-excluded {
  display: block;
  margin-top: var(--fono-space-1);
  color: var(--fono-warning);
}
.history-analytics-state {
  font-size: var(--fono-type-sm);
  line-height: 1.6;
}
</style>
