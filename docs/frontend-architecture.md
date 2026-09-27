# Frontend architecture: Fono UI v2 and v3

## V3 preview (Vue)

V3 lives in `src/v3/` and starts from `v3.html`. It uses Vue 3, TypeScript,
PrimeVue unstyled and the private `@whitelife-core/ui-kit` package, following
the setup in WhiteLife: `createWlPt()`, `wlLocaleRu`, then explicit reset, base
and theme CSS imports. `.npmrc` selects GitHub Packages for this scope; npm
credentials remain outside the repository.

`src/v3/styles/tokens.css` is the V3 customization layer. Fono semantic colors
refer to the UI kit's `--wl-*` theme tokens; a 4px spacing scale, type sizes and
major layout dimensions are defined there. Component CSS uses those variables
so palette, density and layout can be adjusted centrally.

The V3 dictation feature separates pure domain data, a runtime port,
Tauri/mock infrastructure and Vue presentation. Native access goes through
`src/lib/ipc.ts`. The browser at `http://127.0.0.1:1420/v3.html` shows demo
data. `npm run dev:desktop:v3` starts a Tauri development build with V3 in the
settings window and the existing V2 overlay. The regular app config and
installer continue to open V2 until the new UI is accepted.

V3 is an initial voice workspace, not a replacement for the full settings,
commands, service and trainer pages. Its “All settings” link opens the existing
V2 interface. Keep these pages available while V3 grows feature by feature.

## Status and goal

Fono is a local Windows application built with Tauri 2, React, TypeScript and
Vite. The stable UI v1 is currently in `src/views`, `src/components` and
`src/lib`. UI v2 is a parallel redesign: it must use the same Rust runtime and
the same typed IPC contract, without changing stable v1 as a side effect.

The purpose of this document is to make UI v2 easier to evolve, test and
eventually replace v1 with. It is a target architecture for new UI work, not a
requirement to mechanically rewrite the current frontend.

## What remains shared

There is **one** Tauri application and one Rust core. UI v2 must not become a
second Tauri application: two apps would compete for microphone capture,
global shortcuts, tray state, overlay windows and persisted settings.

The native core remains the authority for the dictation pipeline, wake word,
STT worker lifecycle, text injection, global shortcuts and window lifecycle.
The UI receives state through Tauri commands and events.

```text
Rust core and Tauri commands/events
                 |
        typed frontend boundary
                 |
        +--------+--------+
        |                 |
   stable UI v1       parallel UI v2
```

## UI v2 directory structure

New UI code belongs under `src/v2/`. Do not begin by moving or editing v1
components. The exact list of features may grow, but imports must follow these
boundaries:

```text
src/
  lib/                         # legacy v1 contract until it is migrated
  views/                       # stable UI v1
  components/                  # stable UI v1
  v2/
    app/                       # app composition, routes, providers
    shared/
      domain/                  # cross-feature pure types and functions
      infrastructure/          # Tauri adapters and local persistence
      presentation/            # reusable UI primitives, tokens, icons
      testing/                 # test factories and test helpers
    features/
      dictation/
        domain/
        application/
        infrastructure/
        presentation/
      wake-word/
      transcription-history/
      commands/
      settings/
      onboarding/
      overlay/
```

## UI-only development mode

Use `npm run dev:ui` and open `http://localhost:1420/?ui=v2` to work on UI v2
without starting Tauri, Rust workers, a microphone or global shortcuts. This
route renders `src/v2/app/UiV2App.tsx` with a typed mock runtime.

The mock is a development adapter, not a second source of product logic. A
feature first depends on its narrow runtime port; later, the mock adapter is
replaced by a Tauri IPC adapter that uses the existing typed boundary. The
stable v1 interface remains the default route, and `?ui=v2` is an explicit
opt-in until the migration is accepted.

`shared/` is not a dumping ground. Move a component, type or utility there
only when at least two features need it. A feature may use another feature's
explicit public API, but must not import its internal files.

## Layers

### Domain

`domain` contains pure, deterministic frontend logic and types. It must not
import React, Tauri, browser APIs or storage. Examples: mapping pipeline states
to user-facing UI states, validating a settings draft, deriving countdown view
data and transcript presentation rules.

### Application

`application` owns use-cases, feature hooks and feature-local state. It
coordinates domain logic with ports defined by the feature. A page gets data
and callbacks from a feature hook instead of implementing behavior inside JSX.

### Infrastructure

`infrastructure` implements external boundaries: Tauri `invoke`, Tauri event
subscriptions, window operations, local storage and, in the future, HTTP
clients. It exposes typed adapters to application code.

For Fono, `src/lib/ipc.ts` is the current centralized Tauri boundary. New v2
work may wrap or gradually split it by feature, but it must not duplicate raw
`invoke()` and `listen()` calls inside components.

### Presentation

`presentation` contains pages, components, styles and view models. Components
are functional and responsible for rendering and composition only. Data flows
through props and callbacks. A page owns state through its feature hook;
global state is reserved for truly long-lived app state such as the current
pipeline snapshot and settings cache.

## Fono-specific boundaries

- Tauri commands are narrow typed adapters for OS/native capabilities such as
  dictation control, microphone enumeration, overlay position and app-window
  operations.
- Do not invoke Tauri APIs directly from React components.
- Do not move UI-specific rules into Rust merely because Tauri commands exist.
  Conversely, do not reimplement Rust-owned pipeline, wake-word or injection
  rules in TypeScript; the UI renders the backend state it receives.
- Any new native command needs an explicit TypeScript type, minimal input and
  output surface, and the least privileges necessary. UI must never gain broad
  file-system or shell access for convenience.
- The overlay is a separate Tauri webview. Changes to its UI must preserve its
  event subscriptions, position persistence and lifecycle independently of the
  main settings/dashboard window.

## Current and future external services

Fono currently uses local Tauri IPC for the application core and an optional
Rust-side HTTP client for LM Studio. It has no frontend `/api/*` contract,
authentication, workspaces, notes, tasks, calendar, board or comments service.
Those concepts must not be added as speculative frontend state.

If a future service is added:

1. define its real backend/API contract and typed configuration first;
2. add one centralized client or adapter in `infrastructure`;
3. add types and a feature use-case;
4. only then add navigation and UI.

For a web mode, preserve the same-origin `/api/*` contract and centralize the
base URL. For Tauri, native OS operations remain Tauri adapters; backend
services use their typed client rather than hardcoded URLs in UI components.
Avoid trailing slashes on paths where the service redirects them.

## Future command agent and AI actions

The current command system is local and limited. If a future LM Studio command
agent can perform actions, reads may run immediately, but every write must use:

```text
preview -> explicit user confirmation -> execute
```

AI action cards should be compact, typed presentation components inside their
own interaction surface. Do not reuse full settings forms or expose raw JSON
outside debug/details. Never present local speculative state as though a
server-side capability exists.

## Size, tests and temporary workarounds

- A component, hook or class has one responsibility. Before a file reaches
  roughly 300 lines, split it into meaningful units.
- Keep tests next to the unit under test: domain functions, application hooks
  and Tauri adapters are tested separately from rendering.
- The repository does not yet expose a unit-test script. Before adding the
  first interactive UI v2 feature, introduce a lightweight TypeScript test
  runner (for example Vitest) and add the corresponding `npm run test` command;
  do not let the absence of a runner become a reason to leave new domain logic
  or hooks untested.
- Before handing off UI work, run unit tests when present, `npm run lint` and
  `npm run build`, then smoke-test both the main and overlay windows in Tauri.
- Every temporary workaround must have a `TODO(YYYY-MM-DD):` comment and a
  matching entry in the technical-debt section of the relevant feature doc or
  issue.

## Incremental migration plan

1. Define UI v2 information architecture and visual tokens using mock data.
2. Add `src/v2/app` and one feature at a time, starting with the dashboard and
   dictation state display.
3. Connect each feature to the existing typed IPC boundary after its mock
   interaction is agreed.
4. Rebuild the overlay as a separately tested v2 feature.
5. Switch the default UI only after v2 covers the needed v1 behavior and has
   been tested against the real release binary.
6. Remove v1 only in a dedicated change after the replacement is accepted.
