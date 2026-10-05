# Архитектура фронтенда Fono

Единственный рабочий интерфейс находится в `src/v3/`: Vue 3, TypeScript, Vue Router 4 и WhiteUI 0.6.0. Старые React-интерфейсы и автономные HTML-прототипы удалены; прежние реализации доступны в истории Git.

## Точки входа и runtime

`index.html` и `v3.html` запускают `src/v3/main.ts`. Основное окно Tauri использует `v3.html`, индикатор — `v3.html?view=overlay`. Hash-маршруты задаются в `src/v3/app/router.ts`.

Composition root выбирает native runtime только внутри Tauri. В обычном браузере подключается mock runtime с демонстрационными данными; микрофон, глобальная клавиша и вставка в Windows там не проверяются.

```text
src/v3/
  app/              # composition roots, router и оболочка
  features/         # диктовка, настройки, история, тренер, команды, API, обновления
  shared/domain/    # общие типизированные порты и чистые правила
  shared/application/
  shared/infrastructure/native/  # IPC, события и DTO ipcTypes.ts
  shared/presentation/
  styles/           # токены и общие стили
```

## Слои

- `domain`: типы и чистые преобразования без Vue, Tauri и браузерных API.
- `application`: сценарии, состояние и координация через внедрённые порты.
- `infrastructure`: адаптеры Tauri IPC, событий, clipboard, storage и focus.
- `presentation`: Vue-компоненты, композиция и отображение.

Фичи используют публичные API друг друга; общие элементы появляются после второго потребителя. Сложные правила остаются вне шаблонов. Файлы около 300 строк разделяются по ответственности.

Rust управляет захватом аудио, моделью, worker-процессами, горячими клавишами, вставкой и жизненным циклом индикатора. Компоненты не вызывают `invoke`, `listen`, browser storage или window API напрямую. IPC DTO находятся в `shared/infrastructure/native/ipcTypes.ts`; контракт UI-портов — в `shared/domain/contracts.ts`.

## Стили и состояние

Порядок стилей: WhiteUI reset → base → тема → Fono. Тема устанавливается на `html`, чтобы Teleport-диалоги и списки наследовали её. Цвета, плотность, размеры и движение задаются в `styles/tokens.css` через цветовые роли `--wl-*`.

Настройки и история native runtime сохраняются через Rust. Последний результат сессии хранится отдельно от архива. Browser mock сохраняет только разрешённые демонастройки; тексты, инструкции и ключи остаются в памяти.

Переключатели применяются сразу с ожиданием и откатом при ошибке. Остальные параметры используют общий черновик, сохранение, отмену и защиту несохранённых правок.

## Индикатор и предпросмотр

Рабочий общий компонент — `features/overlay/presentation/OverlayIndicator.vue`. Его используют native overlay и браузерный предпросмотр `#/overlay`. Отдельный набросок удалён. `#/onboarding` и `#/scenarios` остаются рабочими средствами настройки и проверки.

Native overlay сохраняет `focus: false` и `focusable: false`. Обработка, завершение и выбор параметров привязаны к идентификатору сессии; запоздалый ответ не изменяет новую сессию.

## Проверки

`npm run typecheck:v3`, `npm run lint`, `npm run test:v3` (также `test:ui`), `npm run test:release`, `npm run format:check`, `npm run build`, `npm run version:check`.

Автоматические тесты находятся в `tests/ui/`; native Windows gate — в `scripts/test-native.ps1`. Пользователь самостоятельно проверяет основное и overlay-окно, голос, hotkey, вставку и установку. Browser mock и unit tests не являются приёмкой установленной сборки.

Подробности: [интерфейс](fono-v3-frontend.md), [разработка](development.md), [текущий статус](STATUS.md).
