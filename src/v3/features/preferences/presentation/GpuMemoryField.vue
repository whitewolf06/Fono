<script setup lang="ts">
import { computed } from "vue";
import type { Preferences } from "../../../shared/domain/contracts";
import { gpuMemoryUsage } from "../../../shared/domain/gpuMemory";
import { useWorkspace } from "../../../shared/application/workspace";
import SelectField from "../../../shared/presentation/SelectField.vue";

const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const status = computed(() => workspace.state.gpuMemory);
const usage = computed(() => gpuMemoryUsage(status.value));
const options = [
  { value: "resident", label: "Постоянно · быстрый старт" },
  { value: "adaptive", label: "Адаптивно · освобождать при нехватке памяти" },
];
</script>
<template>
  <div class="form-stack">
    <SelectField
      id="gpu-memory"
      v-model="draft.gpuModelResidency"
      label="Модель в видеопамяти"
      :options="options"
      :disabled="draft.acceleration === 'cpu'"
      :hint="
        draft.acceleration === 'cpu'
          ? 'При ускорении на процессоре видеопамять не используется. Выбранный режим сохранится для GPU.'
          : draft.gpuModelResidency === 'adaptive'
            ? 'Освобождает модель Whisper при нехватке видеопамяти только в простое. Следующая диктовка стартует медленнее из-за загрузки модели.'
            : 'Модель Whisper остаётся в памяти между диктовками. Подходит, когда важен быстрый старт.'
      "
    />
    <p v-if="workspace.native && status" class="notice" aria-live="polite">
      Сейчас: {{ status.message }}<span v-if="usage"> · {{ usage }}</span>
    </p>
    <small v-else class="muted">
      {{
        workspace.state.gpuMemoryError ||
        (workspace.native
          ? "Получаем состояние видеопамяти…"
          : "Браузерный макет: видеопамять не измеряется.")
      }}
    </small>
    <small v-if="draft.gpuModelResidency === 'adaptive'" class="muted">
      Мониторинг пока поддерживается на Windows с одной видеокартой и выделенной
      видеопамятью. На остальных системах модель остаётся загруженной. Во время
      распознавания она не выгружается. Модели внешнего ИИ этот режим не
      затрагивает.
    </small>
  </div>
</template>
