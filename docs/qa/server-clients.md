# Application key provisioning acceptance — 2026-11-06

Candidate: Fono **0.8.42**, with application provisioning in the existing
standalone server and Vue/WhiteUI server panel. No consumer application or
production server configuration was changed.

## Local automated checks

| Check                       | Result                                                                                                                                                                                                                                            |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Native Linux server suite   | 136 passed, 1 existing public-inference fixture test ignored; Rust 1.96.1, isolated offline Linux container                                                                                                                                       |
| Native key lifecycle        | Admin-only create/list/revoke, authorization before body reads, bounded JSON and deadline, no scope escalation, immediate authorization/revocation, metadata-only inventory, durable reopen, concurrent writes and protected admin records passed |
| Panel controller/HTTP/SSR   | 24 passed, including one-time secret clearing, stale-session results, duplicate clicks, URL/environment injection, copy failure and administrative endpoint isolation                                                                             |
| Browser fixture             | 47 checks passed across Chromium 720×560, 900×720 and 1440×900; all three screenshots reviewed                                                                                                                                                    |
| Browser actions             | Six fixture keys issued, three confirmed revocations; double-click sends one create, actual browser clipboard matches the snippet, non-admin receives 403, no revocation before confirmation                                                      |
| Browser privacy/layout      | Empty local/session storage, cookies and IndexedDB; secrets clear on navigation/sign-out; one form/tab; no horizontal overflow, external requests, runtime or CSP errors                                                                          |
| Frontend/static gates       | Typecheck, lint, production build, formatting, version synchronization, documentation links and embedded panel freshness passed                                                                                                                   |
| Release/CI policy contracts | 53 passed; 2 server container publication-policy tests passed                                                                                                                                                                                     |

The full V3 regression suite passed **487/487** tests. Strict Linux Clippy also
passed for all server targets with `-D warnings`.
The release run includes 32 repository policy tests and 21 pre-existing
working-tree cache-maintenance tests; the latter are outside this feature commit.

The Linux test container had no network access, source files were read-only,
and Git, local tracker configuration and build data were masked. Only public
runtime libraries and crates.io packages were used. The pinned official Clippy
component was separately downloaded and SHA-256 checked before transfer.

Evidence is local under `build/`: `fono-clients-native-tests.log`,
`fono-clients-release-tests.log`, and `server-clients-browser-qa/result.json` with
three `applications-*.png` screenshots. Fixture browser tests exercise the real
embedded panel against a controlled HTTP API. Native router/token-store tests
exercise the actual Rust implementation; browser fixtures do not prove inference
or production network reachability.

## Production and owner checks still required

1. Deploy the new Fono Server binary/image, preserving existing tokens and models.
2. Ensure only Fono's credential directory is writable by the service account;
   retain the read-only root filesystem and other existing restrictions.
3. Sign in to the actual panel with an admin token. Set `server.client_url` once
   or enter a consumer-reachable API origin; include its hostname in the Host
   allowlist. Verify the route from the intended consuming runtime separately.
4. Create a temporary application key and copy the `.env` snippet. Verify the
   key can call authenticated discovery/recognition and cannot manage keys.
5. Revoke that key, verify the next authenticated request receives 401, then
   restart Fono and verify another intentionally retained key still works.

This record does not claim deployment on Kari, a public release, owner
production acceptance, CUDA requalification or integration changes in Kvorum.
Setup and HTTP details are in [application connections](../server-clients.md).
