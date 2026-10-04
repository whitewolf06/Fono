<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import type { WakeValidationKind } from "../../../shared/domain/wake";
import { WAKE_WORD_AVAILABLE } from "../../../shared/domain/wakeAvailability";
const props = defineProps<{ phrase: string; language: "ru" | "en" }>();
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const result = ref("");
const setup = computed(() => workspace.state.wakeSetup);
const changed = computed(
  () =>
    props.phrase !== workspace.state.preferences.wakePhrase ||
    props.language !== workspace.state.preferences.wakeLanguage,
);
const reasons: Record<string, string> = {
  silence: "слишком тихо",
  clipping: "перегрузка микрофона",
  too_short: "фраза слишком короткая",
  phrase_not_detected: "фраза не обнаружена",
};
const reason = computed(
  () => reasons[setup.value?.latest?.reason || ""] || "фраза не подтверждена",
);
async function savePhrase() {
  await workspace.settings.save({
    wakePhrase: props.phrase.trim(),
    wakeLanguage: props.language,
  });
}
async function begin() {
  await run(async () => {
    if (workspace.state.preferences.wakeEnabled)
      await workspace.settings.toggle("wakeEnabled", false);
    await savePhrase();
    await workspace.wake!.load();
    if (!workspace.state.wakeSetup?.modelReady)
      await workspace.wake!.download();
    await workspace.wake!.begin();
  });
}
async function validate(kind: WakeValidationKind) {
  await run(() => workspace.wake!.validate(kind));
}
onMounted(() => {
  if (WAKE_WORD_AVAILABLE) void run(() => workspace.wake!.load());
});
</script>
<template>
  <section
    v-if="workspace.wake && WAKE_WORD_AVAILABLE"
    class="wake-setup form-stack"
    aria-label="Настройка своей фразы"
  >
    <p class="muted">
      {{
        workspace.native
          ? "Пять повторов подбирают чувствительность. Три свежих повтора, тишина и похожая фраза проверяют результат. Запись начинается только кнопкой."
          : "Демонстрация мастера: записи и результаты подготовлены, микрофон не используется."
      }}
    </p>
    <p v-if="setup?.verified && !changed" class="notice" role="status">
      Профиль проверен. Фраза «{{ setup.phrase }}» готова к включению.
    </p>
    <WlButton
      v-if="
        setup?.verified && !changed && !workspace.state.preferences.wakeEnabled
      "
      size="sm"
      :disabled="busy"
      @click="run(() => workspace.settings.toggle('wakeEnabled', true))"
      >Включить пробуждение</WlButton
    >
    <div class="actions">
      <WlButton size="sm" :disabled="busy || !phrase.trim()" @click="begin">{{
        setup?.profileReady ? "Настроить заново" : "Настроить свою фразу"
      }}</WlButton>
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="busy || !setup?.modelReady || changed"
        @click="
          run(async () => {
            result = await workspace.wake!.test();
          })
        "
        >Проверить голосом</WlButton
      >
    </div>
    <div v-if="setup?.active" class="surface-panel form-stack">
      <h4>1 · Произнесите «{{ setup.phrase }}»</h4>
      <progress
        :value="setup.accepted"
        :max="setup.required"
        :aria-label="'Принято ' + setup.accepted + ' из ' + setup.required"
      />
      <p>
        {{ setup.accepted }} из {{ setup.required }} повторов принято. Говорите
        обычным голосом.
      </p>
      <p
        v-if="setup.latest && !setup.latest.accepted"
        class="error-text"
        role="status"
      >
        Образец отклонён: {{ reason }}. Повторите.
      </p>
      <WlButton
        size="sm"
        :loading="busy"
        :disabled="changed"
        @click="run(() => workspace.wake!.record())"
        >Записать следующий повтор · 4 сек</WlButton
      >
    </div>
    <div
      v-if="setup?.profileReady && !setup.active && !setup.verified && !changed"
      class="surface-panel form-stack"
    >
      <h4>2 · Проверьте на свежих записях</h4>
      <p>Записи этого шага не используются для подбора чувствительности.</p>
      <WlButton
        v-if="!setup.validation.active"
        size="sm"
        :disabled="busy"
        @click="run(() => workspace.wake!.beginValidation())"
        >{{
          setup.validation.failed ? "Повторить проверку" : "Начать проверку"
        }}</WlButton
      >
      <template v-else>
        <p>
          Фраза: {{ setup.validation.positivePassed }}/{{
            setup.validation.positiveRequired
          }}
          · Тишина:
          {{ setup.validation.silencePassed ? "пройдена" : "ожидается" }} ·
          Похожая фраза:
          {{ setup.validation.otherPhrasePassed ? "пройдена" : "ожидается" }}
        </p>
        <div class="actions">
          <WlButton
            size="sm"
            :disabled="
              busy ||
              setup.validation.positivePassed >=
                setup.validation.positiveRequired
            "
            @click="validate('positive')"
            >Произнести фразу</WlButton
          >
          <WlButton
            size="sm"
            :disabled="busy || setup.validation.silencePassed"
            @click="validate('silence')"
            >Записать тишину</WlButton
          >
          <WlButton
            size="sm"
            :disabled="busy || setup.validation.otherPhrasePassed"
            @click="validate('other_phrase')"
            >Похожая фраза</WlButton
          >
        </div>
        <p class="muted">
          Для последней проверки произнесите похожие слова, не произнося саму
          фразу пробуждения.
        </p>
      </template>
      <p v-if="setup.validation.failed" class="error-text" role="alert">
        Профиль не прошёл проверку. Повторите настройку или выберите более
        различимую фразу.
      </p>
    </div>
    <div v-if="setup?.active || setup?.validation.active" class="actions">
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="busy"
        @click="run(() => workspace.wake!.cancel())"
        >Отменить настройку</WlButton
      >
    </div>
    <p v-if="changed && (setup?.active || setup?.profileReady)" class="notice">
      Фраза изменена. Начните настройку заново для нового значения.
    </p>
    <p v-if="busy" role="status">
      Подготовка модели или запись и проверка образца…
    </p>
    <p v-if="result" class="notice" role="status">{{ result }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </section>
  <WlButton v-else size="sm" disabled>Настройка фразы · скоро</WlButton>
</template>
