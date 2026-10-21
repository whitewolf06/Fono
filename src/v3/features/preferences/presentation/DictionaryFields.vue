<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

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
    <h3>{{ $t("ui.4e7867b548") }}</h3>
    <PreferenceToggle
      name="dictionaryEnabled"
      :label="$t('ui.3c2325c46e')"
      :description="$t('ui.bccad88ecb')"
    />
    <p class="muted">{{ $t("ui.15ded707e7") }}</p>
    <PreferenceToggle
      name="dictionaryLlmEnabled"
      :label="$t('ui.9fe22dbb16')"
      :description="$t('ui.cff15c6357')"
    />
    <p class="muted">{{ $t("ui.b7fd507ca8") }}</p>
    <p v-if="!workspace.native" class="notice">{{ $t("ui.470561427e") }}</p>
    <ul
      v-if="draft.dictionaryEntries.length"
      class="dictionary-list"
      :aria-label="$t('ui.ebae558908')"
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
            :aria-label="$t('ui.669958f765') + entry.written"
            @click="editing = index"
          >
            <template #icon><AppIcon name="edit" /></template
            >{{ $t("ui.b8b151d417") }}
          </WlButton>
          <WlButton
            size="sm"
            variant="ghost"
            :aria-label="$t('ui.1c6789d896') + entry.written"
            @click="remove(index)"
            >{{ $t("ui.be99b13612") }}</WlButton
          >
        </div>
      </li>
    </ul>
    <p v-else class="notice">{{ $t("ui.3f6c724a00") }}</p>
    <div class="actions">
      <WlButton
        size="sm"
        :disabled="draft.dictionaryEntries.length >= dictionaryLimits.entries"
        @click="editing = -1"
        >{{ $t("ui.f16553632e") }}</WlButton
      >
      <WlButton
        v-if="!draft.dictionaryEntries.length"
        size="sm"
        variant="ghost"
        @click="example"
        >{{ $t("ui.8fe6b9dc59") }}</WlButton
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
