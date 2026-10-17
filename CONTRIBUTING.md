# Contributing to Fono

Fono is a Windows x64 Tauri application with a Vue 3 frontend. Start with the
[README](README.md), [documentation index](docs/README.md), and
[architecture](docs/architecture.md). Existing architectural decisions are in
[docs/adr](docs/adr/README.md).

## Choose a development path

- **Frontend:** install Node.js 22, run `npm ci`, then `npm run dev:ui`.
  Open `http://127.0.0.1:1420/v3.html`. This uses demo data and never proves
  microphone, hotkey, Tauri IPC, or Windows insertion behavior.
- **CPU desktop:** follow [development.md](docs/development.md) and run
  `npm run dev:cpu`. This uses real embedded Whisper, speech VAD and native
  devices, with an isolated debug data directory. GPU SDKs are not required.
- **GPU or packaging:** use the separate prerequisites and resource preparation
  in the development guide. A normal contribution does not require an installer.

Dependency installation follows `package-lock.json` and uses the public npm
registry without credentials. The original WhiteUI **0.6.0** archive is included
in [vendor/whiteui](vendor/whiteui/README.md), with its origin and hashes. Fork
pull requests run the frontend checks without secrets. Do not commit npm tokens,
API credentials, signing keys, audio models, Cargo targets or local profiles.
Read [dependency-policy.md](docs/dependency-policy.md) before changing packages
or vendored components. Keep the effective WhiteUI version at **0.6.0**.

## Before submitting

Run the checks relevant to the change:

```sh
npm run typecheck:v3
npm run lint
npm run test:v3
npm run test:release
npm run test:cpu
npm run docs:check
npm run format:check
npm run version:check
npm run build
```

Native changes also require the Rust workspace tests, strict Clippy and affected
feature configurations in [testing.md](docs/testing.md). Tests use pinned
Sherpa/ONNX resources, not a random system runtime. Report which commands ran
and which Windows scenarios remain untested. Do not substitute mock results
for native acceptance. The [QA index](docs/qa/README.md) describes those scenarios.

## Versions and commits

`package.json` is the source of the app version. Each new commit increments the
patch exactly once and includes `[vX.Y.Z]` in its subject. Use the repository
commands so all five application manifests and lock entries stay synchronized:

```sh
npm run hooks:install
npm run version:bump -- patch
npm run version:check
```

Stage the intended code and version files together. Internal Rust crate versions
are independent. Minor versions, publication and installation are separate
owner decisions. Do not publish packages or releases as part of a routine PR.

## Keep changes reviewable

Explain the concrete problem, resulting behavior, validation and remaining
limits. Preserve operation IDs, cancellation and resource ownership. Vue
components use typed ports rather than Tauri IPC or browser storage directly.
Long or blocking I/O belongs outside the UI thread; update activity leases stay
alive until background work actually ends.

For defects, provide steps, app version, model/backend and a sanitized technical
report. Public issues must not contain private dictations or credentials.
Report vulnerabilities using [SECURITY.md](SECURITY.md).
