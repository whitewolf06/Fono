# Server observability acceptance — 2026-10-30

Candidate: Fono **0.8.28**, API 2, worker protocol 4. Native inputs are based on
`1cabe4e` plus SRV-10. Host: Ubuntu 24.04.5, RTX 3060 Ti, driver 595.84.
Build/runtime bases use Ubuntu 22.04 and CUDA 12.8.1.

## Artifact identity

- Source archive SHA-256: `380641ac89e481e5494c03fe223482746552257a3cb35cb723f83ea2098a8b32`.
- Local image ID: `sha256:e6935194c21445c62d99458b35424368f75db07ace00b9862a9f178bbc967797`.
- Server SHA-256: `3a58f438ce4490b2c70f8c4f12c1e2007ebad7550df9fe745f3f685c20f3096f`.
- CUDA worker SHA-256: `c804378a65d5cf10c1cc82e4174090b7d09dafcd0a6bdadb90826afef8ba02c0`.

Docker Hub's authentication endpoint repeatedly timed out while resolving the
floating Dockerfile frontend tag. The successful local build used Docker's
bundled frontend through a temporary copy of `Dockerfile.cuda` with only its
first `# syntax` line removed. Pinned base images, Cargo inputs and
`packaging/docker/build-cuda.sh` remained the same. No daemon, driver, DNS or
TLS settings were changed. The temporary Dockerfile was removed afterward.
This is local build evidence; GitHub Actions execution remains separate.

## Results

| Check                                                             | Observed result                                                                                                                                        |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Native server/shared contracts                                    | 262 passed; 3 existing external-fixture tests ignored                                                                                                  |
| Strict Clippy                                                     | Server, STT, service, types and release CUDA worker passed                                                                                             |
| CPU synchronous recognition and file job                          | Both completed; 2 native windows and 2 successful RTF observations                                                                                     |
| CUDA synchronous recognition, file job, worker crash and recovery | 4 attempts; 3 successful RTF observations; failed worker attempt counted; next request recovered                                                       |
| Real driver memory                                                | CUDA inventory available with nonzero observed free memory; memory series absent without an available GPU                                              |
| Metrics access                                                    | Recognition credential rejected with 403; admin credential received metrics                                                                            |
| Histograms                                                        | Increasing cumulative bucket counts; `+Inf` equals `_count`; finite observed values                                                                    |
| JSON logs                                                         | 50 CPU and 25 CUDA records in this run; every line parsed as JSON and matched the allowlisted fields                                                   |
| Privacy                                                           | Actual test credentials, private query/header/request-ID markers, filename, temporary paths and public transcription text absent from logs and metrics |
| Shutdown                                                          | Both test containers exited with code 0 and emitted `stopped`                                                                                          |
| Unit observation boundaries                                       | Batch preemption counted separately from retained successful windows; cancellation released active/waiting gauges and added no success RTF             |
| Log backpressure                                                  | A blocked writer left producers running; excess records incremented the drop counter                                                                   |
| Log configuration                                                 | Supported TOML/environment levels validated; invalid values rejected without echoing them                                                              |
| Exact CPU API smoke command from workflow                         | Fresh temporary model storage; doctor speech probe passed; health/discovery/recognize 200, anonymous discovery 401                                     |
| Frontend/release regressions                                      | Typecheck, lint, build, 412 V3 tests, 52 release tests and 2 publication-policy tests passed                                                           |
| Repository checks                                                 | Formatting, version synchronization and documentation links passed                                                                                     |

Tests used [pinned public speech](../evidence/licenses/server-smoke-NOTICE.md),
read-only mounts, dropped capabilities and the image's UID 10001 for the live
telemetry scenarios. No microphone or personal recording was used. Counts of
HTTP log records depend on readiness/job polling and are observations rather
than a fixed protocol requirement.

All temporary containers, credentials, CDI specifications and hooks were
removed. The local test image and source/build artifacts remain available for
subsequent validation. Existing host services were not restarted or replaced.

## Limits

This pass verifies telemetry against actual CPU/CUDA events and deterministic
tests. It does not establish sustained throughput, long-running soak behavior,
LLM memory budgets or production acceptance. Native TLS and certificate reload
remain SRV-11 work. Publication and permanent deployment have not occurred.

The [observability contract](../server-observability.md) defines units, scopes,
privacy rules, failure handling and the HTTP duration histogram migration.
