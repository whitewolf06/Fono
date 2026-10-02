<script setup lang="ts">
import { computed, ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const { run } = useFeedback();
const revealed = ref(false);
const token = "fono-demo-token-not-a-real-secret";
const address = computed(
  () => "http://" + (workspace.state.serviceAddress || "127.0.0.1:17832"),
);
const example =
  'curl -X POST http://127.0.0.1:17832/v1/transcriptions\n  -H "Authorization: Bearer YOUR_TOKEN"\n  -F "audio=@recording.wav"';
const endpoints = [
  ["GET", "/v1/health", "Доступность и текущая модель"],
  ["POST", "/v1/transcriptions", "Создать задачу распознавания"],
  ["GET", "/v1/transcription-jobs/{id}", "Получить состояние и результат"],
  ["POST", "/v1/transcription-jobs/{id}/cancel", "Отменить задачу"],
  ["GET", "/openapi.json", "Спецификация OpenAPI"],
];
</script>
<template>
  <div class="form-stack">
    <section class="surface-panel">
      <h2>Локальное подключение</h2>
      <p v-if="workspace.native">
        API принимает аудиофайлы на этом компьютере. Для запросов нужен
        Bearer-токен.
      </p>
      <p v-else>
        Приложение обращается к Fono на этом компьютере. В браузерном макете
        запросы не отправляются.
      </p>
      <div class="copy-row">
        <code>{{ address }}</code
        ><WlButton
          size="sm"
          @click="run(() => workspace.copy(address), 'Адрес скопирован')"
          >Копировать адрес</WlButton
        >
      </div>
      <div class="section-divider" />
      <h3>Bearer-токен</h3>
      <small v-if="!workspace.native" class="muted"
        >Это демонстрационный токен. Настоящие ключи в макет вводить не
        нужно.</small
      >
      <div class="copy-row">
        <code>{{
          !workspace.native && revealed ? token : "••••••••••••••••••••••••"
        }}</code
        ><WlButton
          v-if="!workspace.native"
          size="sm"
          variant="ghost"
          @click="revealed = !revealed"
          ><template #icon
            ><AppIcon :name="revealed ? 'eye-off' : 'eye'" /></template
          >{{ revealed ? "Скрыть" : "Показать" }}</WlButton
        ><WlButton
          size="sm"
          @click="
            run(
              () =>
                workspace.service.copyToken
                  ? workspace.service.copyToken()
                  : workspace.copy(token),
              'Токен скопирован',
            )
          "
          >Копировать</WlButton
        >
      </div>
    </section>
    <section class="surface-panel">
      <div class="section-header">
        <h2>Пример запроса</h2>
        <WlButton
          size="sm"
          variant="ghost"
          @click="
            run(
              () =>
                workspace.copy(
                  example.replace('http://127.0.0.1:17832', address),
                ),
              'Пример скопирован',
            )
          "
          ><template #icon><AppIcon name="copy" /></template
          >Копировать</WlButton
        >
      </div>
      <pre>{{ example.replace("http://127.0.0.1:17832", address) }}</pre>
      <p class="muted">
        Ответ содержит идентификатор задачи. Запрашивайте её состояние, пока не
        появится результат.
      </p>
    </section>
    <section class="surface-panel">
      <h2>Методы API</h2>
      <div
        v-for="[method, path, description] in endpoints"
        :key="path"
        class="endpoint-row"
      >
        <span class="method" :class="{ post: method === 'POST' }">{{
          method
        }}</span>
        <div>
          <code>{{ path }}</code>
          <p>{{ description }}</p>
        </div>
      </div>
      <div class="copy-row">
        <span>Полная документация: <code>/docs</code></span
        ><WlButton
          size="sm"
          @click="
            run(
              () => workspace.copy(address + '/docs'),
              'Ссылка на документацию скопирована',
            )
          "
          >Копировать ссылку</WlButton
        >
      </div>
    </section>
  </div>
</template>
