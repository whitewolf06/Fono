<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import { useInteraction } from "../../../shared/application/interaction";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { computed } from "vue";
import { engineNames } from "../../../shared/domain/speechModels";
import { measurementContext } from "../../../shared/domain/sttMeasurements";
const workspace = useWorkspace();
const { run } = useFeedback();
const ui = useInteraction();
const groups = computed(() =>
  (Object.keys(engineNames) as (keyof typeof engineNames)[])
    .map((engine) => ({
      engine,
      title: engineNames[engine],
      models: workspace.state.models.filter(
        (m) => (m.engine || "whisper") === engine,
      ),
    }))
    .filter((group) => group.models.length),
);
const readiness = computed(() => {
  const status = workspace.state.sttReadiness;
  if (!workspace.native || !status) return "";
  if (status.state === "loading") return "Распознавание: загружаем модель…";
  if (status.state === "ready")
    return (
      "Распознавание готово · " + (status.device || "устройство не указано")
    );
  if (status.state === "failed") return "Модель не готова: " + status.message;
  return "Модель распознавания не загружена";
});
async function remove(id: string) {
  if (
    await ui.confirm({
      title: "Удалить модель?",
      text:
        "Чтобы снова использовать модель, потребуется загрузить её." +
        (workspace.native
          ? " Файл модели будет удалён с компьютера."
          : " В демо файлы не меняются."),
      accept: "Удалить модель",
      danger: true,
    })
  )
    await run(() => workspace.settings.removeModel(id));
}
</script>
<template>
  <section class="model-manager">
    <h3>Модели на компьютере</h3>
    <p class="muted">
      {{
        workspace.native
          ? "Модели хранятся локально. Для удаления активной модели сначала выберите другую."
          : "Загрузка и объём показаны для демонстрации."
      }}
    </p>
    <p v-if="readiness" class="muted" role="status">{{ readiness }}</p>
    <p class="muted">
      {{ measurementContext }}
    </p>
    <p class="muted">
      Это небольшой набор для сравнения. Ваш голос и компьютер могут дать другой
      результат. RTF 0,5 означает 5 секунд распознавания на 10 секунд записи;
      загрузка модели, подготовка аудио, обработка текста и вставка сюда не
      входят.
    </p>
    <section
      v-for="group in groups"
      :key="group.engine"
      :aria-label="group.title"
    >
      <h4>{{ group.title }}</h4>
      <div v-for="model in group.models" :key="model.id" class="model-row">
        <AppIcon name="inbox" />
        <div>
          <strong>{{ model.name }}</strong>
          <p>{{ model.description }} · {{ model.size }}</p>
          <p v-if="model.benchmark" class="muted">{{ model.benchmark }}</p>
          <p v-if="model.languages">
            Языки: {{ model.languages.join(", ").toUpperCase()
            }}{{ model.multilingual ? " и другие" : "" }} · Пунктуация:
            {{ model.punctuation ? "есть" : "нет" }}
          </p>
          <p v-if="model.license">
            <a :href="model.licenseUrl" target="_blank" rel="noreferrer">{{
              model.license
            }}</a>
            · {{ model.engine === "whisper" ? "CPU / доступное GPU" : "CPU"
            }}<span v-if="workspace.state.preferences.model === model.id">
              · Выбрана</span
            >
          </p>
          <p
            v-if="
              model.downloadPhase === 'verifying' &&
              model.status === 'downloading'
            "
            role="status"
          >
            Проверяем пакет…
          </p>
          <p v-if="model.downloadError" class="error-text" role="alert">
            {{ model.downloadError }}
          </p>
          <progress
            v-if="model.status === 'downloading'"
            :value="model.progress"
            max="100"
            :aria-label="'Загрузка ' + model.name"
          />
        </div>
        <WlButton
          v-if="model.status === 'available'"
          size="sm"
          @click="run(() => workspace.settings.downloadModel(model.id))"
          ><template #icon><AppIcon name="download" /></template
          >Загрузить</WlButton
        >
        <WlButton
          v-else-if="model.status === 'downloading'"
          size="sm"
          variant="ghost"
          @click="run(() => workspace.settings.removeModel(model.id))"
          >Отменить {{ model.progress }}%</WlButton
        >
        <WlButton
          v-else-if="!model.custom"
          size="sm"
          variant="ghost"
          :disabled="workspace.state.preferences.model === model.id"
          :aria-label="'Удалить ' + model.name"
          @click="remove(model.id)"
          ><template #icon><AppIcon name="trash" /></template>Удалить</WlButton
        >
      </div>
    </section>
  </section>
</template>
