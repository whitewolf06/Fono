<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import PreferenceToggle from "../../preferences/presentation/PreferenceToggle.vue";
import SnippetEditor from "./SnippetEditor.vue";
import {
  snippetLimits,
  validateSnippets,
  type Snippet,
} from "../domain/snippets";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace(),
  ui = useInteraction();
const editing = ref<number | null>(null),
  busy = ref(false),
  error = ref(""),
  notice = ref("");
function store(entry: Snippet) {
  const entries = draft.value.snippets.map((entry) => ({ ...entry }));
  if (editing.value === -1) entries.push(entry);
  else if (editing.value !== null) entries[editing.value] = entry;
  draft.value.snippets = entries;
  editing.value = null;
}
async function transfer(importing: boolean) {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  notice.value = "";
  try {
    if (importing) {
      const entries = await workspace.snippetTransfer.import();
      if (entries === null) return;
      const problem = validateSnippets(entries);
      if (problem) throw new Error(problem);
      if (
        draft.value.snippets.length &&
        !(await ui.confirm({
          get title() {
            return uiMessage("ui.b069bef7de");
          },
          get text() {
            return uiMessage("ui.056e15738f", { v0: String(entries.length) });
          },
          get accept() {
            return uiMessage("ui.c20163fda1");
          },
        }))
      )
        return;
      draft.value.snippets = entries.map((entry) => ({ ...entry }));
      notice.value = uiMessage("ui.f14ef0d36b", { v0: String(entries.length) });
    } else if (await workspace.snippetTransfer.export(draft.value.snippets))
      notice.value = uiMessage("ui.14544e240c");
  } catch (value) {
    error.value =
      value instanceof Error ? value.message : uiMessage("ui.4549599dac");
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <section id="snippets" class="form-stack snippet-settings">
    <div class="section-divider" />
    <h3>{{ $t("ui.1f438e7197") }}</h3>
    <PreferenceToggle
      name="snippetsEnabled"
      :label="$t('ui.d6d122461b')"
      :description="$t('ui.df64a5d1a4')"
    />
    <p class="muted">{{ $t("ui.4db9c38937") }}</p>
    <p v-if="!workspace.native" class="notice">{{ $t("ui.42c67e8811") }}</p>
    <ul
      v-if="draft.snippets.length"
      class="snippet-list"
      :aria-label="$t('ui.1f438e7197')"
    >
      <li v-for="(entry, index) in draft.snippets" :key="index">
        <div>
          <strong>{{ entry.trigger }}</strong
          ><small
            >{{ entry.enabled ? $t("ui.0b81629905") : $t("ui.5a577d9056") }} ·
            {{ entry.text }}</small
          >
        </div>
        <div class="actions">
          <WlButton
            size="sm"
            variant="ghost"
            :aria-label="$t('ui.4c0f79aebb') + entry.trigger"
            @click="editing = index"
            >{{ $t("ui.b8b151d417") }}</WlButton
          ><WlButton
            size="sm"
            variant="ghost"
            :aria-label="$t('ui.124c8f004c') + entry.trigger"
            @click="
              draft.snippets = draft.snippets.filter((_, i) => i !== index)
            "
            >{{ $t("ui.be99b13612") }}</WlButton
          >
        </div>
      </li>
    </ul>
    <p v-else class="notice">{{ $t("ui.a16538caa0") }}</p>
    <div class="actions">
      <WlButton
        size="sm"
        :disabled="busy || draft.snippets.length >= snippetLimits.entries"
        @click="editing = -1"
        >{{ $t("ui.4ee6262fe4") }}</WlButton
      ><WlButton
        size="sm"
        variant="ghost"
        :disabled="busy"
        @click="transfer(true)"
        >{{ $t("ui.011008fbaa") }}</WlButton
      ><WlButton
        size="sm"
        variant="ghost"
        :disabled="busy"
        @click="transfer(false)"
        >{{ $t("ui.361d70696a") }}</WlButton
      ><small class="muted"
        >{{ draft.snippets.length }} / {{ snippetLimits.entries }}</small
      >
    </div>
    <p class="muted">{{ $t("ui.b08dd91038") }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <SnippetEditor
      v-if="editing !== null"
      :entry="
        editing === -1
          ? { trigger: '', text: '', enabled: true }
          : draft.snippets[editing]
      "
      :entries="draft.snippets"
      :index="editing"
      @save="store"
      @close="editing = null"
    />
  </section>
</template>
<style scoped>
.snippet-list {
  display: grid;
  gap: var(--fono-space-3);
  padding: 0;
  list-style: none;
}
.snippet-list li {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--fono-space-3);
  border-bottom: 1px solid var(--fono-border);
  padding-bottom: var(--fono-space-3);
}
.snippet-list li > div:first-child {
  min-width: 0;
  flex: 1;
}
.snippet-list strong,
.snippet-list small {
  display: block;
  overflow-wrap: anywhere;
}
.snippet-list small {
  margin-top: var(--fono-space-1);
  color: var(--fono-muted);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.snippet-settings > .muted {
  font-size: var(--fono-type-sm);
}
</style>
