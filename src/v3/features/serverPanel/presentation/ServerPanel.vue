<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { WlButton, WlField, WlInput } from "@whitelife-core/ui-kit";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { useServerPanel } from "../application/panel";
import { formatBytes, stateLabel, type PanelTab } from "../domain/panel";

const panel = useServerPanel();
const state = panel.state;
const credential = ref("");
const search = ref("");
const endpoint = ref("GET /v1/health");
const model = ref("");
const language = ref("auto");
const format = ref("json");
const timestamps = ref("none");
const tabs: { id: PanelTab; label: string }[] = [
  { id: "status", label: "Статус" },
  { id: "models", label: "Модели" },
  { id: "queue", label: "Очередь" },
  { id: "clients", label: "Приложения" },
  { id: "docs", label: "API и проверка" },
];
const endpointOptions = [
  { value: "GET /healthz", label: "GET /healthz — готовность" },
  { value: "GET /v1/health", label: "GET /v1/health — статус" },
  {
    value: "GET /v1/capabilities",
    label: "GET /v1/capabilities — возможности",
  },
  { value: "GET /v1/models", label: "GET /v1/models — модели" },
  { value: "GET /openapi.json", label: "GET /openapi.json — спецификация" },
  { value: "POST /v1/recognize", label: "POST /v1/recognize — аудиофайл" },
  {
    value: "POST /v1/audio/transcriptions",
    label: "POST /v1/audio/transcriptions — OpenAI",
  },
];
const modelOptions = computed(() =>
  state.models.map((entry) => ({ value: entry.id, label: entry.id })),
);
const operations = computed(() =>
  state.operations.filter((operation) =>
    `${operation.path} ${operation.summary}`
      .toLowerCase()
      .includes(search.value.toLowerCase()),
  ),
);
const audioRequest = computed(() => endpoint.value.startsWith("POST "));
const openai = computed(() =>
  endpoint.value.endsWith("/v1/audio/transcriptions"),
);
const responseText = computed(() =>
  state.response?.data
    ? JSON.stringify(state.response.data, null, 2)
    : (state.response?.text ?? ""),
);
watch(
  () => state.models,
  (models) => {
    if (!models.some((entry) => entry.id === model.value))
      model.value =
        models.find((entry) => entry.fono.default)?.id ?? models[0]?.id ?? "";
  },
);
function signIn() {
  const value = credential.value;
  credential.value = "";
  void panel.login(value);
}
function run() {
  const [method, path] = endpoint.value.split(" ");
  void panel.run({
    method: method as "GET" | "POST",
    path,
    model: model.value,
    language: language.value,
    format: format.value,
    timestamps: timestamps.value,
  });
}
onUnmounted(() => panel.dispose());
</script>

<template>
  <main class="server-panel">
    <header class="panel-header">
      <div class="panel-brand">
        <span class="panel-brand-icon"
          ><AppIcon name="server" :size="24"
        /></span>
        <div>
          <h1>Fono Server</h1>
          <p>Распознавание речи · {{ state.connection }}</p>
        </div>
      </div>
      <div v-if="state.authenticated" class="panel-actions">
        <span class="panel-version">v{{ state.capabilities?.version }}</span>
        <WlButton severity="secondary" size="sm" @click="panel.logout()"
          >Выйти</WlButton
        >
      </div>
    </header>

    <section
      v-if="!state.authenticated"
      class="panel-card panel-login"
      aria-labelledby="login-title"
    >
      <span class="panel-eyebrow">ПАНЕЛЬ СЕРВЕРА</span>
      <h2 id="login-title">Подключитесь к Fono</h2>
      <p class="panel-muted">
        Введите API-токен этого сервера. После входа доступны статус, модели,
        очередь и проверка API.
      </p>
      <form class="panel-form" @submit.prevent="signIn">
        <WlField id="server-token" v-slot="field" label="Токен доступа">
          <WlInput
            v-bind="field"
            v-model="credential"
            type="password"
            autocomplete="off"
            :spellcheck="false"
            placeholder="Вставьте токен"
            :disabled="state.signingIn"
          />
        </WlField>
        <p v-if="state.error" class="panel-error" role="alert">
          {{ state.error }}
        </p>
        <WlButton
          type="submit"
          :loading="state.signingIn"
          :disabled="!credential.trim() || state.signingIn"
          >Войти</WlButton
        >
      </form>
      <p class="panel-footnote">
        <AppIcon name="shield" />Токен хранится в памяти вкладки. После
        перезагрузки страницы войдите снова.
      </p>
    </section>

    <template v-else>
      <nav class="panel-tabs" aria-label="Разделы панели">
        <WlButton
          v-for="tab in tabs"
          :key="tab.id"
          :severity="state.tab === tab.id ? 'primary' : 'secondary'"
          :class="{ 'panel-tab-active': state.tab === tab.id }"
          :aria-current="state.tab === tab.id ? 'page' : undefined"
          @click="state.tab = tab.id"
          >{{ tab.label }}</WlButton
        >
      </nav>
      <div class="panel-toolbar">
        <span class="panel-muted">{{
          state.refreshing
            ? "Обновляем данные…"
            : "Данные получены с этого сервера"
        }}</span>
        <div class="panel-actions">
          <WlButton
            size="sm"
            severity="secondary"
            :aria-pressed="state.autoRefresh"
            @click="panel.setAutoRefresh(!state.autoRefresh)"
          >
            {{
              state.autoRefresh
                ? "Автообновление: вкл."
                : "Автообновление: выкл."
            }}</WlButton
          >
          <WlButton
            size="sm"
            severity="secondary"
            :loading="state.refreshing"
            @click="panel.refresh(true)"
            >Обновить</WlButton
          >
        </div>
      </div>
      <p v-if="state.error" class="panel-message panel-error" role="alert">
        {{ state.error }}
      </p>

      <section v-if="state.tab === 'status'" aria-label="Статус сервера">
        <div class="panel-stats">
          <article class="panel-card">
            <span class="panel-eyebrow">СОСТОЯНИЕ</span
            ><strong
              class="panel-stat"
              :class="{ 'panel-good': state.health?.status === 'ready' }"
              >{{ stateLabel(state.health?.status ?? "—") }}</strong
            ><span class="panel-muted">Готовность модели по умолчанию</span>
          </article>
          <article class="panel-card">
            <span class="panel-eyebrow">МОДЕЛИ В ПАМЯТИ</span
            ><strong class="panel-stat">{{
              state.health?.models.filter((entry) =>
                ["ready", "busy"].includes(entry.state),
              ).length ?? "—"
            }}</strong
            ><span class="panel-muted"
              >{{ state.models.length }} в списке сервера</span
            >
          </article>
          <article class="panel-card">
            <span class="panel-eyebrow">ЗАПРОСЫ</span
            ><strong class="panel-stat">{{
              state.health
                ? state.health.queue.interactive + state.health.queue.batch
                : "—"
            }}</strong
            ><span class="panel-muted"
              >Ожидание диктовки и пакетная работа</span
            >
          </article>
        </div>
        <div class="panel-columns">
          <article class="panel-card">
            <h2>Устройства</h2>
            <ul class="panel-list">
              <li
                v-for="device in state.capabilities?.devices"
                :key="device.id"
              >
                <AppIcon :name="device.kind === 'cuda' ? 'chart' : 'server'" />
                <div>
                  <strong>{{ device.name || device.id }}</strong
                  ><span class="panel-muted"
                    >{{ device.id
                    }}<template v-if="device.memory_total_mb">
                      · {{ device.memory_total_mb }} МиБ</template
                    ></span
                  >
                </div>
              </li>
            </ul>
          </article>
          <article class="panel-card">
            <h2>Возможности</h2>
            <dl class="panel-facts">
              <dt>Версия API / протокол воркера</dt>
              <dd>
                {{ state.capabilities?.api_version }} /
                {{ state.capabilities?.worker_protocol }}
              </dd>
              <dt>Длительность аудио</dt>
              <dd>до {{ state.capabilities?.max_audio_seconds }} с</dd>
              <dt>Размер файла</dt>
              <dd>
                {{ formatBytes(state.capabilities?.max_upload_bytes ?? 0) }}
              </dd>
              <dt>OpenAI-совместимый API</dt>
              <dd>
                {{
                  state.capabilities?.features.openai_compatible
                    ? "Включён"
                    : "Выключен"
                }}
              </dd>
              <dt>Метрики</dt>
              <dd>
                {{
                  state.capabilities?.features.metrics
                    ? "Включены · admin-токен"
                    : "Выключены"
                }}
              </dd>
            </dl>
          </article>
        </div>
      </section>

      <section
        v-if="state.tab === 'models'"
        class="panel-card"
        aria-labelledby="models-title"
      >
        <h2 id="models-title">Модели сервера</h2>
        <p class="panel-muted">
          Установленные модели и их текущее состояние. Загрузка файлов
          выполняется через CLI сервера.
        </p>
        <div v-if="state.models.length" class="panel-table-scroll">
          <table class="panel-table">
            <thead>
              <tr>
                <th>Модель</th>
                <th>Состояние</th>
                <th>Устройство</th>
                <th>Размер</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="entry in state.models" :key="entry.id">
                <td>
                  <strong>{{ entry.id }}</strong
                  ><small
                    >{{ entry.fono.engine }} ·
                    {{ entry.fono.languages.join(", ") || "—"
                    }}<template v-if="entry.fono.default">
                      · По умолчанию</template
                    ></small
                  >
                </td>
                <td>
                  {{ stateLabel(entry.fono.state)
                  }}<small>{{ stateLabel(entry.fono.disk_state) }}</small>
                </td>
                <td>{{ entry.fono.device }}</td>
                <td>{{ formatBytes(entry.fono.size_bytes) }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <p v-else class="panel-empty">
          Сервер не вернул установленных моделей.
        </p>
      </section>

      <section v-if="state.tab === 'queue'" aria-label="Очередь запросов">
        <div class="panel-columns">
          <article class="panel-card">
            <span class="panel-eyebrow">ДИКТОВКА</span
            ><strong class="panel-stat">{{
              state.health?.queue.interactive ?? "—"
            }}</strong>
            <h2>Интерактивные запросы</h2>
            <p class="panel-muted">
              Ожидают распознавания. Они получают приоритет на границе текущего
              пакетного окна.
            </p>
          </article>
          <article class="panel-card">
            <span class="panel-eyebrow">ПАКЕТНАЯ РАБОТА</span
            ><strong class="panel-stat">{{
              state.health?.queue.batch ?? "—"
            }}</strong>
            <h2>Задания и транскрибация</h2>
            <p class="panel-muted">
              Сумма ожидающих, подготавливаемых и распознаваемых заданий.
            </p>
          </article>
        </div>
        <p class="panel-footnote">
          Здесь показаны общие счётчики. Получить результат конкретного задания
          можно по его ID через API.
        </p>
      </section>

      <section
        v-if="state.tab === 'clients'"
        class="panel-docs"
        aria-label="Приложения и ключи API"
      >
        <article class="panel-card">
          <h2>Подключить приложение</h2>
          <p class="panel-muted">
            Создайте отдельный ключ и скопируйте адрес с токеном в .env
            приложения. Ключ разрешает распознавание, но не управление сервером.
          </p>
          <p
            v-if="state.clientError"
            class="panel-error panel-message"
            role="alert"
          >
            {{ state.clientError }}
          </p>
          <p v-if="state.clientsAllowed === false" class="panel-muted">
            Обычный токен подходит для распознавания. Создавать и отзывать ключи
            может администратор.
          </p>
          <form
            v-else
            class="panel-form"
            @submit.prevent="panel.createClient()"
          >
            <WlField
              id="client-api-url"
              v-slot="field"
              label="Адрес API для приложения"
            >
              <WlInput
                v-bind="field"
                v-model="state.clientUrl"
                placeholder="http://fono-server:17832"
                :spellcheck="false"
                :disabled="state.clientsBusy || !!state.clientEnv"
              />
            </WlField>
            <p class="panel-muted">
              Укажите адрес, доступный из самого приложения. Для программы на
              сервере это может быть http://127.0.0.1:17832. В Docker localhost
              указывает внутрь контейнера: нужен доступный сетевой адрес Fono.
              Адрес панели через SSH-туннель может отличаться.
            </p>
            <WlField id="client-name" v-slot="field" label="Имя приложения">
              <WlInput
                v-bind="field"
                v-model="state.clientName"
                placeholder="Например, my-app"
                autocomplete="off"
                :spellcheck="false"
                :disabled="state.clientsBusy || !!state.clientEnv"
              />
            </WlField>
            <p class="panel-muted">
              До 64 латинских букв, цифр, дефисов и подчёркиваний. Начните с
              буквы или цифры.
            </p>
            <WlButton
              type="submit"
              :loading="state.clientsBusy"
              :disabled="
                !state.clientsAllowed ||
                state.clientsBusy ||
                !!state.clientEnv ||
                !state.clientName.trim() ||
                !state.clientUrl
              "
              >Создать ключ</WlButton
            >
          </form>
          <div v-if="state.clientEnv" class="panel-client-secret">
            <h3>Ключ для {{ state.issuedClient }}</h3>
            <p class="panel-muted">
              Сохраните обе строки. При смене раздела, выходе или перезагрузке
              страницы ключ будет скрыт. На сервере хранится только его хэш.
            </p>
            <pre tabindex="0" aria-label="Настройки подключения для .env">{{
              state.clientEnv
            }}</pre>
            <div class="panel-actions">
              <WlButton @click="panel.copyClientEnvironment()"
                >Скопировать .env</WlButton
              >
              <WlButton severity="secondary" @click="panel.clearClientSecret()"
                >Скрыть ключ</WlButton
              >
            </div>
          </div>
          <p v-if="state.clientNotice" class="panel-footnote" role="status">
            {{ state.clientNotice }}
          </p>
        </article>
        <article class="panel-card">
          <div class="panel-actions">
            <h2>Ключи приложений</h2>
            <WlButton
              severity="secondary"
              size="sm"
              :loading="state.clientsBusy"
              :disabled="state.clientsBusy"
              @click="panel.loadClients()"
              >Обновить список</WlButton
            >
          </div>
          <p
            v-if="
              state.clientsLoaded &&
              !state.clients.length &&
              state.clientsAllowed
            "
            class="panel-empty panel-muted"
          >
            Ключей приложений пока нет.
          </p>
          <ul class="panel-list panel-client-list">
            <li v-for="client in state.clients" :key="client.name">
              <div>
                <strong>{{ client.name }}</strong
                ><span>Распознавание · создан {{ client.created_at }}</span>
              </div>
              <WlButton
                severity="secondary"
                size="sm"
                :disabled="state.clientsBusy"
                @click="panel.askRevoke(client.name)"
                >Отозвать ключ</WlButton
              >
            </li>
          </ul>
          <div
            v-if="state.revokeCandidate"
            class="panel-client-confirm"
            role="alert"
          >
            <p>
              Отозвать ключ приложения
              <strong>{{ state.revokeCandidate }}</strong
              >? Следующие запросы с этим ключом будут отклонены.
            </p>
            <div class="panel-actions">
              <WlButton
                :disabled="state.clientsBusy"
                @click="panel.revokeClient()"
                >Подтвердить отзыв</WlButton
              >
              <WlButton
                severity="secondary"
                :disabled="state.clientsBusy"
                @click="panel.cancelRevoke()"
                >Отмена</WlButton
              >
            </div>
          </div>
        </article>
      </section>

      <section
        v-if="state.tab === 'docs'"
        class="panel-docs"
        aria-label="Документация и пробный запрос"
      >
        <article class="panel-card">
          <h2>Пробный запрос</h2>
          <p class="panel-muted">
            Запрос отправляется на этот сервер с вашим токеном. Для
            распознавания выберите аудиофайл.
          </p>
          <div class="panel-form-grid">
            <SelectField
              id="panel-endpoint"
              v-model="endpoint"
              label="Эндпоинт"
              :options="endpointOptions"
              :disabled="state.requestBusy"
            />
            <template v-if="audioRequest">
              <SelectField
                id="panel-model"
                v-model="model"
                label="Модель"
                :options="modelOptions"
                :disabled="state.requestBusy"
              />
              <SelectField
                id="panel-language"
                v-model="language"
                label="Язык"
                :options="[
                  { value: 'auto', label: 'Определить автоматически' },
                  { value: 'ru', label: 'Русский' },
                  { value: 'en', label: 'English' },
                ]"
                :disabled="state.requestBusy"
              />
              <SelectField
                v-if="openai"
                id="panel-format"
                v-model="format"
                label="Формат ответа"
                :options="
                  ['json', 'text', 'srt', 'vtt', 'verbose_json'].map(
                    (value) => ({ value, label: value }),
                  )
                "
                :disabled="state.requestBusy"
              />
              <SelectField
                v-else
                id="panel-timestamps"
                v-model="timestamps"
                label="Временные метки"
                :options="[
                  { value: 'none', label: 'Без меток' },
                  { value: 'segments', label: 'По сегментам' },
                  { value: 'words', label: 'По словам' },
                ]"
                :disabled="state.requestBusy"
              />
            </template>
          </div>
          <div v-if="audioRequest" class="panel-file">
            <WlButton
              severity="secondary"
              :disabled="state.requestBusy"
              @click="panel.chooseAudio()"
              ><AppIcon name="audio-file" />Выбрать аудиофайл</WlButton
            >
            <span>{{
              state.audio
                ? `${state.audio.name} · ${formatBytes(state.audio.size)}`
                : "Файл не выбран"
            }}</span>
            <small class="panel-muted"
              >{{
                openai
                  ? "WAV, MP3, FLAC, Ogg/Vorbis, Ogg/Opus"
                  : "WAV или Ogg/Opus"
              }}
              · до
              {{
                formatBytes(state.capabilities?.max_upload_bytes ?? 0)
              }}</small
            >
          </div>
          <p v-if="state.requestError" class="panel-error" role="alert">
            {{ state.requestError }}
          </p>
          <div class="panel-actions">
            <WlButton
              :loading="state.requestBusy"
              :disabled="
                state.requestBusy || (audioRequest && (!state.audio || !model))
              "
              @click="run"
              >Отправить запрос</WlButton
            >
            <WlButton
              v-if="state.requestBusy"
              severity="secondary"
              @click="panel.cancelRequest()"
              >Отменить ожидание</WlButton
            >
          </div>
          <div v-if="state.response" class="panel-response" aria-live="polite">
            <div class="panel-response-meta">
              <strong
                :class="
                  state.response.status < 400 ? 'panel-good' : 'panel-error'
                "
                >HTTP {{ state.response.status }}</strong
              ><span>{{ state.response.elapsedMs }} мс</span
              ><code v-if="state.response.requestId">{{
                state.response.requestId
              }}</code>
            </div>
            <pre tabindex="0">{{ responseText }}</pre>
          </div>
          <p class="panel-footnote">
            Файл отправляется только по кнопке. Ответ остаётся в памяти вкладки
            и очищается при выходе.
          </p>
        </article>
        <article class="panel-card">
          <h2>Документация API v2</h2>
          <p class="panel-muted">
            Маршруты и схемы получены из OpenAPI этого сервера. TLS, токены и
            лимиты действуют и для панели.
          </p>
          <WlField id="panel-search" v-slot="field" label="Найти эндпоинт"
            ><WlInput
              v-bind="field"
              v-model="search"
              placeholder="Например, /v1/models"
          /></WlField>
          <p v-if="!state.operations.length" class="panel-muted">
            Спецификация пока недоступна. Перезайдите, чтобы загрузить её снова.
          </p>
          <details
            v-for="operation in operations"
            :key="`${operation.method} ${operation.path}`"
            class="panel-operation"
          >
            <summary>
              <span class="panel-method">{{ operation.method }}</span
              ><code>{{ operation.path }}</code
              ><span>{{ operation.summary }}</span>
            </summary>
            <p>{{ operation.description || operation.summary }}</p>
            <WlButton
              v-if="operation.runnable"
              severity="secondary"
              size="sm"
              @click="endpoint = `${operation.method} ${operation.path}`"
              >Выбрать для проверки</WlButton
            >
            <pre tabindex="0">{{
              JSON.stringify(operation.schema, null, 2)
            }}</pre>
          </details>
          <details class="panel-operation">
            <summary>Полная спецификация OpenAPI</summary>
            <pre tabindex="0">{{ state.specification }}</pre>
          </details>
        </article>
      </section>
    </template>
    <footer class="panel-footer">
      Fono Server · Ubuntu API
      <span>Панель работает с реальными данными сервера</span>
    </footer>
  </main>
</template>
