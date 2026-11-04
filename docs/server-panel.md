# Fono Server web panel

Open the running server's `/` or `/panel/` address in a browser. Use the same
HTTP/HTTPS endpoint, port, certificate trust and Host allowlist as the API.
The panel is embedded in the native binary and CUDA image; it needs no Node
runtime, CDN, additional port or separate frontend service.

## Sign in and privacy

Enter a token created by `fono-server token create <name>`. A recognition token
is sufficient for the panel. The login request reads `/v1/capabilities`; it does
not create a server session or exchange the token for a cookie. All subsequent
data requests use the existing bearer authorization and token scope checks.

Only the login shell and its fixed JavaScript/CSS files are public. `/docs`,
`/openapi.json`, model inventory and detailed health remain authenticated.
The panel never stores the token, selected file or response in local/session
storage, IndexedDB, cookies, URLs or a service worker. Sign-out, page reload and
page navigation clear the session; an observed 401 also clears authenticated
data and stops polling. The selected audio file is uploaded only after clicking
the request button. Transcripts remain visible in this tab until cleared,
replaced, signed out or reloaded.

Requests are restricted to this server's known same-origin routes, reject
redirects, omit browser cookies and bypass caches. Responses set `no-store`.
Panel assets set CSP, frame protection, no-referrer and restrictive browser
permissions; the panel never records a microphone. See the browser contracts
for [redirect handling](https://developer.mozilla.org/en-US/docs/Web/API/Request/redirect)
and [frame-ancestors](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/frame-ancestors).

## Screens and actual data

- **Status:** default-model readiness, loaded models, version/API/worker protocol,
  observed CPU/CUDA devices and configured request limits. A 503 health response
  during model startup is displayed as starting, without rejecting a valid token.
- **Models:** `/v1/models` inventory, default marker, live state, placement, size,
  languages and disk observation. `present_unverified` does not claim that a fresh
  SHA check was performed. Model installation and removal remain CLI actions.
- **Queue:** `/v1/health` aggregate counters. `interactive` counts waiting
  interactive requests; `batch` includes queued, preparing and transcribing jobs.
  The API does not provide a global per-job list. Existing jobs are accessed by
  their explicit ID through the documented API.
- **API and test:** the actual `/openapi.json` operations, descriptions and schemas,
  plus a request form. Safe discovery GETs and the two synchronous recognition
  endpoints are available; administrative mutations are documentation-only.

Health refreshes every 10 seconds, model inventory every 30 seconds. The user
can pause automatic refresh or request a full manual refresh. Refreshes do not
overlap. A 429 response postpones requests for at least `Retry-After`; quotas
apply exactly as for other API clients. An audio request is never automatically
repeated. Cancel stops waiting and aborts the HTTP request; native work retains
the existing server activity lease until it actually ends.

Native `/v1/recognize` accepts WAV or Ogg/Opus files; the OpenAI-compatible route
also supports MP3, FLAC and Ogg/Vorbis. The form supports the selected installed
model, language, native timestamp mode or OpenAI response format. Server-side
size, duration, format, capability, queue and rate checks remain authoritative.
The result shows HTTP status, elapsed request time, request ID and text/JSON.

## Building and checking

The panel uses Vue 3 and WhiteUI 0.6.0 in `src/v3/features/serverPanel`, with a
dedicated composition root `src/v3/server.ts`. It has an HTTP adapter and does
not import the desktop Tauri runtime or browser mock workspace.

```sh
npm ci
npm run build:server-panel
npm run test:server-panel
npm run check:server-panel
```

Generated assets in `apps/fono-server/resources/panel` are committed so ordinary
Cargo builds remain self-contained and require no private npm access. After
changing panel sources, shared tokens/components or locked dependencies, rebuild
the assets before compiling the server. CI regenerates them in memory and fails
on drift. The manifest records source and output SHA-256; it is not a served
route. The desktop `npm run build` keeps its existing entrypoints and behavior.

The browser panel uses real server requests. It is separate from the desktop's
browser mock and does not confirm Windows microphone, hotkey or insertion UAT.
See the [panel acceptance record](qa/server-panel.md) for the tested Linux
artifact, browser scenarios and remaining owner checks.
