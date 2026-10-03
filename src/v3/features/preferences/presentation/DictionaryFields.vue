<script setup lang="ts">
import { ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type {
  PersonalDictionaryEntry,
  Preferences,
} from "../../../shared/domain/contracts";
import {
  demoDictionaryEntries,
  dictionaryLimits,
} from "../../../shared/domain/personalDictionary";
import { useWorkspace } from "../../../shared/application/workspace";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import DictionaryEntryEditor from "./DictionaryEntryEditor.vue";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const editing = ref<number | null>(null);
function store(entry: PersonalDictionaryEntry) {
  const entries = [...draft.value.dictionaryEntries];
  if (editing.value === -1) entries.push(entry);
  else if (editing.value !== null) entries[editing.value] = entry;
  draft.value.dictionaryEntries = entries;
  editing.value = null;
}
function remove(index: number) {
  draft.value.dictionaryEntries = draft.value.dictionaryEntries.filter(
    (_, i) => i !== index,
  );
}
function example() {
  const entry = demoDictionaryEntries[0];
  if (
    draft.value.dictionaryEntries.some((item) =>
      item.spoken.some((phrase) => entry.spoken.includes(phrase.toLowerCase())),
    )
  )
    return;
  draft.value.dictionaryEntries = [
    ...draft.value.dictionaryEntries,
    { ...entry, spoken: [...entry.spoken] },
  ];
}
</script>
<template>
  <section id="dictionary" class="form-stack dictionary-settings">
    <div class="section-divider" />
    <h3>Личный словарь</h3>
    <PreferenceToggle
      name="dictionaryEnabled"
      label="Использовать мои написания"
      description="Локальные точные замены имён, названий и терминов в итоговом тексте."
    />
    <p class="muted">
      Например, «фоно» → «Fono». Совпадения проверяются по целым словам без
      учёта регистра. Словарь применяется после распознавания и обработки;
      исходная расшифровка остаётся прежней. При выключении записи сохраняются.
    </p>
    <p v-if="!workspace.native" class="notice">
      Браузерная демонстрация: записи действуют до перезагрузки и не сохраняются
      в браузере. В приложении словарь хранится локально.
    </p>
    <ul
      v-if="draft.dictionaryEntries.length"
      class="dictionary-list"
      aria-label="Записи личного словаря"
    >
      <li v-for="(entry, index) in draft.dictionaryEntries" :key="index">
        <div>
          <strong>{{ entry.written }}</strong
          ><small>{{ entry.spoken.join(" · ") }}</small>
        </div>
        <div class="actions">
          <WlButton
            size="sm"
            variant="ghost"
            :aria-label="'Изменить ' + entry.written"
            @click="editing = index"
          >
            <template #icon><AppIcon name="edit" /></template>Изменить
          </WlButton>
          <WlButton
            size="sm"
            variant="ghost"
            :aria-label="'Удалить ' + entry.written"
            @click="remove(index)"
            >Удалить</WlButton
          >
        </div>
      </li>
    </ul>
    <p v-else class="notice">
      Записей пока нет. Добавьте нужное написание и то, как его обычно
      распознаёт модель.
    </p>
    <div class="actions">
      <WlButton
        size="sm"
        :disabled="draft.dictionaryEntries.length >= dictionaryLimits.entries"
        @click="editing = -1"
        >Добавить слово или фразу</WlButton
      >
      <WlButton
        v-if="!draft.dictionaryEntries.length"
        size="sm"
        variant="ghost"
        @click="example"
        >Добавить пример</WlButton
      >
      <small class="muted"
        >{{ draft.dictionaryEntries.length }} /
        {{ dictionaryLimits.entries }}</small
      >
    </div>
    <DictionaryEntryEditor
      v-if="editing !== null"
      :entry="
        editing === -1
          ? { written: '', spoken: [] }
          : draft.dictionaryEntries[editing]
      "
      :entries="draft.dictionaryEntries"
      :index="editing"
      @save="store"
      @close="editing = null"
    />
  </section>
</template>
<style scoped>
.dictionary-list {
  display: grid;
  gap: var(--fono-space-2);
  padding: 0;
  list-style: none;
}
.dictionary-list li {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--fono-space-3);
  border-bottom: 1px solid var(--fono-border);
  padding-bottom: var(--fono-space-3);
}
.dictionary-list li > div:first-child {
  min-width: 0;
  flex: 1;
}
.dictionary-list strong,
.dictionary-list small {
  display: block;
  overflow-wrap: anywhere;
}
.dictionary-list small {
  margin-top: var(--fono-space-1);
  color: var(--fono-muted);
}
.dictionary-settings > .muted {
  font-size: var(--fono-type-sm);
}
</style>
