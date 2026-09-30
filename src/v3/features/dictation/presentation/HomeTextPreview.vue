<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { formatHistoryDate, type VoiceEntry } from "../domain/voice";

const props = defineProps<{
  entry: VoiceEntry | null;
  postProcessingEnabled: boolean;
}>();
const emit = defineEmits<{
  "open-history": [];
  copy: [text: string];
}>();

const editing = ref(false);
const draft = ref("");
const sourceText = computed(
  () => props.entry?.originalText?.trim() || props.entry?.text || "",
);
const hasOriginal = computed(() => Boolean(props.entry?.originalText?.trim()));
const hasChanges = computed(() => draft.value !== sourceText.value);

watch(
  () => props.entry?.id,
  () => {
    draft.value = sourceText.value;
    editing.value = false;
  },
  { immediate: true },
);
watch(
  () => props.postProcessingEnabled,
  (enabled) => {
    if (!enabled) editing.value = false;
  },
);

function toggleEditing() {
  if (!props.entry || !props.postProcessingEnabled) return;
  editing.value = !editing.value;
}

function resetDraft() {
  draft.value = sourceText.value;
  editing.value = false;
}

function copyDraft() {
  if (draft.value.trim()) emit("copy", draft.value);
}
</script>

<template>
  <section class="v3-text-preview" aria-labelledby="v3-text-preview-title">
    <div class="v3-text-preview-heading">
      <div class="v3-text-preview-identity">
        <span class="v3-text-preview-icon" aria-hidden="true">
          <i class="pi pi-comment"></i>
        </span>
        <div>
          <span class="v3-text-preview-eyebrow">Ваши слова</span>
          <h2 id="v3-text-preview-title">Последняя диктовка</h2>
        </div>
      </div>
      <div class="v3-text-preview-actions">
        <button type="button" @click="emit('open-history')">
          <i class="pi pi-history" aria-hidden="true"></i>
          <span>История</span>
        </button>
        <button
          type="button"
          :disabled="!entry || !postProcessingEnabled"
          :title="
            !postProcessingEnabled
              ? 'Доступно при включённой постобработке'
              : !entry
                ? 'Сначала начните диктовку'
                : undefined
          "
          :aria-pressed="editing"
          @click="toggleEditing"
        >
          <i class="pi pi-pencil" aria-hidden="true"></i>
          <span>{{ editing ? "Готово" : "Исправить" }}</span>
        </button>
        <button
          class="v3-text-preview-copy"
          type="button"
          :disabled="!draft.trim()"
          @click="copyDraft"
        >
          <i class="pi pi-copy" aria-hidden="true"></i>
          <span>Копировать</span>
        </button>
      </div>
    </div>
    <div v-if="entry" class="v3-text-preview-meta">
      <time :datetime="entry.createdAt">{{
        formatHistoryDate(entry.createdAt)
      }}</time>
      <span class="v3-text-preview-source">
        <i class="pi pi-circle-fill" aria-hidden="true"></i>
        {{ hasOriginal ? "Исходный текст" : "Результат диктовки" }}
      </span>
    </div>
    <div class="v3-text-preview-field" :class="{ 'is-editing': editing }">
      <textarea
        v-model="draft"
        :aria-label="
          editing ? 'Исправить последний текст' : 'Последний текст диктовки'
        "
        placeholder="После диктовки здесь появится ваш текст."
        :readonly="!editing"
        rows="2"
      ></textarea>
    </div>
    <div v-if="editing || hasChanges" class="v3-text-preview-note">
      <span>Правки остаются в этом поле и не меняют историю.</span>
      <button v-if="hasChanges" type="button" @click="resetDraft">
        Сбросить
      </button>
    </div>
  </section>
</template>
