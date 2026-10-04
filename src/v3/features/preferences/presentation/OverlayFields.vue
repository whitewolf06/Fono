<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
const workspace = useWorkspace();
const { run } = useFeedback();
import { WlSlider, WlButton } from "@whitelife-core/ui-kit";
import type { Phase, Preferences } from "../../../shared/domain/contracts";
import type {
  PendingDictation,
  PendingDictationRequest,
  OverlayProcessingChoice,
} from "../../../shared/domain/processing";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import { OverlayPreview } from "../../overlay";
import SelectField from "../../../shared/presentation/SelectField.vue";
const draft = defineModel<Preferences>({ required: true });
const previewScenario = ref("recording");
const previewFeedback = ref("");
const previewResult = ref<string | null>(null);
let previewRevision = 0;
const sampleText =
  "Проверить новую версию Fono и отправить команде результаты.";
const previewPhase = computed<Phase>(() => {
  const phases: Record<string, Phase> = {
    recording: "listening",
    silence: "silence",
    awaiting_action: "awaiting_action",
    processing: "processing",
    done: "done",
    cancelled: "cancelled",
    error: "error",
  };
  return phases[previewScenario.value] || "listening";
});
const demoPending = computed<PendingDictation | null>(() =>
  previewScenario.value === "awaiting_action"
    ? {
        sessionId: previewRevision,
        phase: "awaiting_action" as const,
        originalText: sampleText,
        resultText: previewResult.value,
        copyOnly: true,
        preset: draft.value.processingMode,
        targetLanguage:
          !draft.value.processingTranslationEnabled ||
          draft.value.processingTranslation === "none"
            ? null
            : draft.value.processingTranslation,
        processingEnabled: draft.value.processingEnabled,
        translationEnabled: draft.value.processingTranslationEnabled,
        source: "hotkey" as const,
        createdAt: "2026-10-04T12:00:00.000Z",
        error: null,
        insertionBlocked: false,
      }
    : null,
);
watch(
  previewScenario,
  () => {
    previewRevision++;
    previewFeedback.value = "";
    previewResult.value = null;
  },
  { flush: "sync" },
);
function resolvePreview(request: PendingDictationRequest) {
  if (demoPending.value?.sessionId !== request.sessionId) return;
  if (request.action === "process_preview") {
    previewResult.value = sampleText;
    previewFeedback.value =
      "Обработка показана на примере. Доступно копирование без вставки в другое приложение.";
    return;
  }
  previewScenario.value = request.action === "cancel" ? "cancelled" : "done";
  previewFeedback.value =
    "Действие показано на примере. Текст не обрабатывался и не вставлялся в другое приложение.";
}
async function copyPreview(text: string) {
  const revision = previewRevision;
  const copied = await run(
    () => workspace.copy(text),
    "Текст примера скопирован",
  );
  if (copied && revision === previewRevision) previewScenario.value = "done";
}
function changePreviewProcessing(choice: OverlayProcessingChoice) {
  draft.value.processingMode = choice.preset;
  draft.value.processingTranslation = choice.targetLanguage ?? "none";
  if (choice.processingEnabled !== undefined)
    draft.value.processingEnabled = choice.processingEnabled;
  if (choice.translationEnabled !== undefined)
    draft.value.processingTranslationEnabled = choice.translationEnabled;
}
</script>
<template>
  <div class="form-stack">
    <PreferenceToggle
      name="overlayEnabled"
      label="Показывать индикатор записи"
      description="Состояние диктовки поверх других приложений."
    /><PreferenceToggle
      name="overlayCompact"
      label="Компактный вид"
      description="Меньше места для статуса. Быстрые настройки остаются доступны."
    />
    <PreferenceToggle
      id="overlayQuickProcessing"
      name="overlayQuickProcessing"
      label="Быстрые настройки обработки"
      description="В режиме повторного нажатия показывать стиль и перевод в индикаторе. Выбор запоминается для следующих диктовок."
    /><label id="overlayScale"
      >Масштаб · {{ draft.overlayScale }}%<WlSlider
        v-model="draft.overlayScale"
        :min="70"
        :max="130"
        :step="5"
        aria-label="Масштаб индикатора" /></label
    ><label
      >Непрозрачность · {{ draft.overlayOpacity }}%<WlSlider
        v-model="draft.overlayOpacity"
        :min="30"
        :max="100"
        :step="5"
        aria-label="Непрозрачность индикатора"
    /></label>
    <SelectField
      v-model="draft.overlayPosition"
      label="Положение"
      :options="[
        { value: 'custom', label: 'Текущее положение' },
        { value: 'bottom', label: 'Внизу по центру' },
        { value: 'top', label: 'Вверху по центру' },
      ]"
    />
    <SelectField
      v-model="previewScenario"
      label="Состояние в примере"
      :options="[
        { value: 'recording', label: 'Запись' },
        { value: 'silence', label: 'Пауза перед завершением' },
        { value: 'awaiting_action', label: 'Распознано · выбор действия' },
        { value: 'processing', label: 'Обработка через ИИ' },
        { value: 'done', label: 'Готово' },
        { value: 'error', label: 'Ошибка' },
        { value: 'cancelled', label: 'Отменено' },
      ]"
      hint="Нажимайте кнопки в примере: они изменяют только предпросмотр."
    />
    <div class="preview-stage" :data-position="draft.overlayPosition">
      <OverlayPreview
        :preferences="draft"
        :phase="previewPhase"
        :pending="demoPending"
        interactive
        @finish="previewScenario = 'awaiting_action'"
        @cancel="previewScenario = 'cancelled'"
        @resolve="resolvePreview"
        @copy="copyPreview"
        @processing-change="changePreviewProcessing"
      /><small
        >Предпросмотр изменяется сразу. Для применения нажмите
        «Сохранить».</small
      >
      <p v-if="previewFeedback" class="notice" role="status">
        {{ previewFeedback }}
      </p>
    </div>
    <details class="advanced" open>
      <summary>Что означают кнопки</summary>
      <div class="form-stack overlay-legend">
        <p class="muted">
          <AppIcon name="check" :size="14" class="dictation-action--insert" />
          Зелёная галочка — завершить запись с текущими настройками и оставить
          результат для копирования. Текст не вставляется автоматически.
        </p>
        <p class="muted">
          <AppIcon
            name="keyboard"
            :size="14"
            class="dictation-action--insert"
          />
          Повторное нажатие горячей клавиши — распознать речь, применить
          выбранную обработку, вставить текст в исходное поле и закрыть
          индикатор.
        </p>
        <p class="muted">
          <AppIcon
            name="sparkle"
            :size="14"
            class="dictation-action--process"
          />
          Обработка — выбранный стиль. «Без изменений» пропускает редактирование
          через ИИ; с переводом выполняется только перевод.
        </p>
        <p class="muted">
          <AppIcon
            name="translate"
            :size="14"
            class="dictation-action--translate"
          />
          Перевод — включить выбранный язык. Шестерёнка справа открывает выбор
          языка. Выключение сохраняет его для следующей записи.
        </p>
        <p class="muted">
          <AppIcon name="x" :size="14" class="dictation-action--cancel" />
          Красный крестик — отменить или закрыть без копирования. «Копировать»
          помещает результат в буфер и закрывает индикатор.
        </p>
        <p class="muted">
          Обработка и перевод доступны при включённом ИИ. При ручном выборе Fono
          ждёт действия после распознавания; новые записи до этого не
          начинаются.
        </p>
        <p class="muted">
          Стиль и язык, выбранные в индикаторе, запоминаются. Быстрые настройки
          можно скрыть; способ запуска обработки остаётся прежним.
        </p>
      </div>
    </details>
    <div class="actions">
      <WlButton
        v-if="workspace.settings.previewOverlay"
        size="sm"
        @click="run(() => workspace.settings.previewOverlay!(draft))"
        >Показать поверх окон</WlButton
      >
      <WlButton
        size="sm"
        @click="
          workspace.settings.resetOverlay
            ? run(() => workspace.settings.resetOverlay!())
            : (draft.overlayPosition = 'bottom')
        "
        >Сбросить положение</WlButton
      ><RouterLink class="text-link" to="/overlay"
        >Все состояния индикатора</RouterLink
      >
    </div>
  </div>
</template>
