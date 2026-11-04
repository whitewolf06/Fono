# Server panel acceptance — 2026-11-04

Candidate: Fono **0.8.30**, based on `815eb6a` plus the server-panel working
changes. Native validation used a local isolated Ubuntu **24.04.5** container,
Rust **1.96.1**, CPU `whisper:tiny` and the
[pinned public speech fixture](../evidence/licenses/server-smoke-NOTICE.md).
No microphone or user desktop application was controlled.

## Artifact identity

- Linux debug server SHA-256: `92affda69bf6aa61a66e374ad0d809141b53160048f9abbbeb2221ddbd4e6f78`.
- Embedded panel source SHA-256: `f63ae3cc1c3838373697ccb8134ced9354b48ab9e2dbbca32c746cd29a0ba2eb`.
- Individual asset SHA-256 values are recorded in
  `apps/fono-server/resources/panel/manifest.json`.

The binary was compiled before the final versioned commit. These hashes identify
the tested snapshot; this record does not claim a published release artifact.

## Observed checks

| Check                           | Result                                                                                                                                                |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Linux native tests              | 273 passed, 3 existing external-fixture tests ignored; server, STT, service and wire types                                                            |
| Strict Linux Clippy             | Server all targets passed with `-D warnings`                                                                                                          |
| V3 regression tests             | 427 passed, including 15 panel controller/HTTP/rendering tests                                                                                        |
| Release and publication policy  | 52 release tests and 2 server publication-policy tests passed                                                                                         |
| Frontend checks                 | Typecheck, lint, production build, formatting, version and documentation links passed                                                                 |
| Embedded asset freshness        | Regenerated in memory and byte-compared; all four files matched                                                                                       |
| Browser layout with fixture API | Chromium: 720×560, 900×720, 1440×900; status, models, queue and documentation; no page-wide horizontal overflow                                       |
| Browser fixture interactions    | Invalid-token error, login, safe GET, WAV upload, observed 401 logout, empty browser storage and no external requests passed                          |
| Real Linux API authorization    | Public shell/JS/CSS returned 200; anonymous detailed health, models, capabilities, docs and OpenAPI returned 401; token-authorized reads returned 200 |
| Real Linux browser              | Chromium 900×720 signed in to the actual native server, read status/models/queue/OpenAPI, and signed out                                              |
| Real audio from browser         | Public WAV through native recognition and OpenAI multipart returned 200 with the expected speech fragment                                             |
| Privacy and HTTP policy         | Token absent from URLs/state snapshots/logs; results cleared at logout; no browser storage/cookies; no-store and panel CSP headers observed           |
| Lifecycle/limits                | Controller tests cover late results after logout/cancel, aborted HTTP, 503 startup, quota backoff and no automatic replay of audio                    |

Browser fixture checks do not prove model inference. The separate real-server
browser check used the actual Linux listener and CPU recognition. GPU device
rendering was exercised with fixture data; the new panel build was not replayed
on Kari/CUDA. Existing [CUDA](server-cuda.md) and [TLS](server-tls.md) acceptance
records describe their own earlier artifacts.

Windows compile/strict-Clippy checks with `DOCS_RS=1` passed. A separate Windows
native test attempt stopped in the existing Whisper CMake build; it is not
reported as a Windows native test pass. Linux native tests ran without
`DOCS_RS`. This task does not change the Windows application flow.

The Linux container, its temporary model/token configuration and browser
session credential were removed after the check. No source snapshot was sent
to Kari; no permanent deployment or publication was performed. Local test logs
and screenshots remain under `build/server-panel-*` and are excluded from Git.

## Owner acceptance

Install/run the new server build on the intended Ubuntu host, then:

1. Open `/panel/` using its configured HTTPS address and trusted certificate.
2. Sign in with a recognition token and verify the actual devices and models.
3. Check queue counters during a request and run a short WAV in the playground.
4. Sign out, reload and verify that sign-in is required and the prior result is gone.

Permanent startup, certificate renewal scheduling, HTTPS browser trust and
owner production acceptance remain separate from this local implementation.
