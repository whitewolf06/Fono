<script setup lang="ts">
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
          title: "Заменить сниппеты в форме?",
          text: `Импортировано записей: ${entries.length}. Текущий список в форме будет заменён; сохранение выполняется отдельной кнопкой.`,
          accept: "Заменить",
        }))
      )
        return;
      draft.value.snippets = entries.map((entry) => ({ ...entry }));
      notice.value = `Добавлено в форму: ${entries.length}. Нажмите «Сохранить».`;
    } else if (await workspace.snippetTransfer.export(draft.value.snippets))
      notice.value = "JSON сохранён. Экспортирован текущий список из формы.";
  } catch (value) {
    error.value =
      value instanceof Error
        ? value.message
        : "Не удалось обработать JSON сниппетов.";
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <section id="snippets" class="form-stack snippet-settings">
    <div class="section-divider" />
    <h3>Сниппеты</h3>
    <PreferenceToggle
      name="snippetsEnabled"
      label="Использовать голосовые сниппеты"
      description="Если вся диктовка совпала с триггером, подставить готовый текст без ИИ. По умолчанию выключено."
    />
    <p class="muted">
      Регистр, лишние пробелы, «ё/е» и точка или восклицательный знак в конце не
      мешают совпадению. Вопрос со знаком «?» и триггер внутри длинной фразы не
      срабатывают. Выключение сохраняет записи.
    </p>
    <p v-if="!workspace.native" class="notice">
      Браузерная демонстрация: сниппеты остаются в памяти до перезагрузки. В
      приложении они сохраняются локально рядом со словарём.
    </p>
    <ul v-if="draft.snippets.length" class="snippet-list" aria-label="Сниппеты">
      <li v-for="(entry, index) in draft.snippets" :key="index">
        <div>
          <strong>{{ entry.trigger }}</strong
          ><small
            >{{ entry.enabled ? "Включён" : "Выключен" }} ·
            {{ entry.text }}</small
          >
        </div>
        <div class="actions">
          <WlButton
            size="sm"
            variant="ghost"
            :aria-label="'Изменить сниппет ' + entry.trigger"
            @click="editing = index"
            >Изменить</WlButton
          ><WlButton
            size="sm"
            variant="ghost"
            :aria-label="'Удалить сниппет ' + entry.trigger"
            @click="
              draft.snippets = draft.snippets.filter((_, i) => i !== index)
            "
            >Удалить</WlButton
          >
        </div>
      </li>
    </ul>
    <p v-else class="notice">
      Сниппетов пока нет. Добавьте подпись, реквизиты или шаблон сообщения.
    </p>
    <div class="actions">
      <WlButton
        size="sm"
        :disabled="busy || draft.snippets.length >= snippetLimits.entries"
        @click="editing = -1"
        >Добавить сниппет</WlButton
      ><WlButton
        size="sm"
        variant="ghost"
        :disabled="busy"
        @click="transfer(true)"
        >Импорт JSON</WlButton
      ><WlButton
        size="sm"
        variant="ghost"
        :disabled="busy"
        @click="transfer(false)"
        >Экспорт JSON</WlButton
      ><small class="muted"
        >{{ draft.snippets.length }} / {{ snippetLimits.entries }}</small
      >
    </div>
    <p class="muted">
      Импорт заменяет список в форме после проверки. Экспорт включает тексты и
      триггеры из формы, без подстановки переменных.
    </p>
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
