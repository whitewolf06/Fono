<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlInput, WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import { filterHistory, groupHistory, timeLabel } from "../application/history";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const ui = useInteraction();
const { run } = useFeedback();
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
      title: "Удалить диктовку?",
      text: "Запись будет удалена из истории. Текущий черновик на главной сохранится.",
      accept: "Удалить",
      danger: true,
    }))
  )
    workspace.history.remove(selected.value.id);
}
async function clear() {
  if (
    await ui.confirm({
      title: "Очистить всю историю?",
      text: "Все архивные диктовки и связанные разборы будут удалены. Последний текст текущей сессии останется на главной.",
      accept: "Очистить историю",
      danger: true,
    })
  )
    workspace.history.clear();
}
</script>
<template>
  <div class="page">
    <PageHeading
      title="История"
      description="Ваши мысли, к которым можно вернуться."
      ><WlButton
        size="sm"
        variant="danger-quiet"
        :disabled="!workspace.state.history.length"
        @click="clear"
        ><template #icon><AppIcon name="trash" :size="16" /></template
        >Очистить</WlButton
      ></PageHeading
    >
    <p v-if="!workspace.state.preferences.historyEnabled" class="notice">
      Сохранение истории выключено. Новые диктовки доступны только в последнем
      тексте на главной.
      <RouterLink to="/settings/privacy">Настроить</RouterLink>
    </p>
    <div class="search-toolbar">
      <div class="search-input">
        <AppIcon name="search" /><WlInput
          v-model="query"
          aria-label="Поиск по истории"
          placeholder="Найти мысль или фразу…"
        />
      </div>
      <SelectField
        v-model="period"
        label="Период"
        :options="[
          { value: 0, label: 'Всё время' },
          { value: 1, label: 'Последние сутки' },
          { value: 7, label: '7 дней' },
          { value: 30, label: '30 дней' },
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
            @click="selectedId = entry.id"
          >
            <span class="entry-title">{{ entry.title }}</span>
            <p>{{ entry.text }}</p>
            <small
              >{{ timeLabel(entry.createdAt) }} ·
              {{ entry.duration }} сек</small
            >
          </button>
        </section>
      </div>
      <article v-if="selected" class="detail-pane">
        <div class="section-header">
          <h2>{{ selected.title }}</h2>
          <small>{{ timeLabel(selected.createdAt) }}</small>
        </div>
        <div class="segmented small">
          <button
            v-if="selected.original"
            :class="{ selected: variant === 'original' }"
            @click="variant = 'original'"
          >
            Исходная расшифровка</button
          ><button
            :class="{ selected: variant === 'result' }"
            @click="variant = 'result'"
          >
            Результат
          </button>
        </div>
        <p v-if="!selected.original" class="muted">
          Исходная расшифровка не сохранена.
        </p>
        <div class="reading-text" tabindex="0">{{ text }}</div>
        <div class="actions">
          <WlButton
            size="sm"
            @click="run(() => workspace.copy(text || ''), 'Текст скопирован')"
            ><template #icon><AppIcon name="copy" /></template
            >Копировать</WlButton
          ><WlButton size="sm" variant="danger-quiet" @click="remove"
            >Удалить запись</WlButton
          >
        </div>
      </article>
    </div>
    <EmptyState
      v-else
      :title="
        workspace.state.history.length ? 'Ничего не найдено' : 'Здесь пока тихо'
      "
      :text="
        workspace.state.history.length
          ? 'Попробуйте другую фразу или период.'
          : 'Начните диктовку — сохранённые записи появятся здесь.'
      "
      ><WlButton
        v-if="query || period"
        size="sm"
        @click="
          query = '';
          period = 0;
        "
        >Сбросить фильтры</WlButton
      ><RouterLink v-else class="text-link" to="/"
        >На главную</RouterLink
      ></EmptyState
    >
  </div>
</template>
