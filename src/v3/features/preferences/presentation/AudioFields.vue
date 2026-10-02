<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { microphones, languages, accelerations } from "../domain/preferences";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectField from "../../../shared/presentation/SelectField.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const props = defineProps<{ scope?: "microphone" | "recognition" }>();
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const installed = computed(() =>
  workspace.state.models
    .filter((m) => m.status === "installed")
    .map((m) => ({ value: m.id, label: m.name })),
);
</script>
<template>
  <div class="form-stack">
    <template v-if="props.scope !== 'recognition'">
      <SelectField
        id="microphone"
        v-model="draft.microphone"
        label="Устройство ввода"
        :options="workspace.state.devices || microphones"
        hint="Системное устройство следует выбору в Windows."
      />
      <div
        v-if="!workspace.state.microphoneAvailable"
        class="notice error"
        role="alert"
      >
        Микрофон не найден. Подключите устройство и повторите проверку.
      </div>
      <div class="signal-test">
        <span
          >Уровень сигнала
          <small v-if="!workspace.native">демонстрация</small></span
        >
        <div
          class="signal-meter"
          role="meter"
          aria-label="Уровень сигнала"
          :aria-valuenow="Math.round(workspace.state.testSignal * 100)"
          aria-valuemin="0"
          aria-valuemax="100"
        >
          <span :style="{ width: workspace.state.testSignal * 100 + '%' }" />
        </div>
      </div>
      <div class="actions">
        <WlButton
          size="sm"
          :loading="busy"
          @click="
            run(
              () => workspace.settings.testMicrophone(draft.microphone),
              workspace.native
                ? 'Проверка микрофона завершена'
                : 'Сигнал в норме · демонстрация',
            )
          "
          ><template #icon><AppIcon name="microphone" /></template>Проверить
          микрофон</WlButton
        ><WlButton
          size="sm"
          variant="ghost"
          :disabled="busy"
          @click="run(() => workspace.settings.playSample())"
          ><template #icon><AppIcon name="play" /></template>Прослушать
          образец</WlButton
        >
      </div>
      <small v-if="!workspace.native" class="muted"
        >В браузерном макете тест и образец синтетические. Доступ к микрофону не
        запрашивается.</small
      >
    </template>
    <template v-if="props.scope !== 'microphone'">
      <SelectField
        id="model"
        v-model="draft.model"
        label="Модель распознавания"
        :options="installed"
        :hint="
          installed.length
            ? 'Работает локально на вашем компьютере.'
            : 'Нет установленных моделей. Загрузите модель ниже.'
        "
      />
      <div class="form-grid">
        <SelectField
          v-model="draft.language"
          label="Язык"
          :options="languages"
        /><SelectField
          v-model="draft.acceleration"
          label="Ускорение"
          :options="workspace.state.accelerations || accelerations"
        />
      </div>
    </template>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </div>
</template>
