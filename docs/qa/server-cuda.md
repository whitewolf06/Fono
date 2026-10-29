# Ubuntu/CUDA container acceptance — 2026-10-29

Candidate: Fono **0.8.26**, API 2, worker protocol 4. The build used the working
tree based on `6e1d1a1`, with the CUDA/Docker changes in this candidate.
Host: Ubuntu 24.04.5, RTX 3060 Ti with 8192 MiB, NVIDIA driver 595.84.
Build and runtime use Ubuntu 22.04 and CUDA 12.8.1.

## Artifact identity

- Source archive SHA-256: `ad8e8d26cf028a8cb839c6523092e369cf75b476233a8507e00447e8e651b822`.
- Local image ID: `sha256:1d1627f9d190f41ba2a3f806303b2e1f3476577f0391aec608e326a64410d283`.
- Server SHA-256: `656b2943a64678586ca2b3ec9c73ca86263b8a1522405acab5fd2c23b8a3e802`.
- CUDA worker SHA-256: `c804378a65d5cf10c1cc82e4174090b7d09dafcd0a6bdadb90826afef8ba02c0`.

The Docker build refreshes workspace source mtimes before Cargo runs. This
prevents a previous snapshot's cached worker from surviving a source replacement
whose preserved timestamps precede compilation. The final negative test below
verified the new CUDA initialization guard in the actual executable.

## Results

| Check                                                 | Observed result                                                                                                             |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Rust server/shared tests                              | 242 passed; 3 existing tests requiring external fixtures ignored                                                            |
| Strict Clippy                                         | Server, STT, service, types and release CUDA worker passed                                                                  |
| Exact CI CPU smoke with fresh temporary model storage | `/healthz` 200, anonymous discovery 401, authenticated capabilities and recognition 200                                     |
| CPU Whisper tiny                                      | Public 11-second WAV recognized; device `cpu`                                                                               |
| CUDA Whisper tiny                                     | Same WAV recognized; device `cuda:0`; worker observed at 434 MiB                                                            |
| CUDA Whisper large turbo                              | Same WAV recognized; device `cuda:0`; worker observed at 2172 MiB                                                           |
| Worker killed during recognition, both GPU models     | `502 worker_failed`; next request 200 on CUDA                                                                               |
| CUDA hidden with `CUDA_VISIBLE_DEVICES=-1`            | Protocol-4 handshake succeeded; model load returned `model_load`, without CPU fallback                                      |
| Compose profile A                                     | UID 10001; root filesystem and credentials read-only; capabilities dropped; Docker health `healthy`; resident turbo on CUDA |
| Compose profile B                                     | GigaAM v3 loaded and ready on CPU as the default model                                                                      |
| Frontend and release regression checks                | Typecheck, lint, build, V3 tests, format, version, documentation links and 52 release tests passed                          |
| Publication policy regression tests                   | 2 passed, including mutations removing release gates                                                                        |

The GPU memory values are driver observations during this fixture, rather than
peak budgets for every model/window/decoding option. Models were verified
against the pinned catalog. Audio came from the
[documented upstream fixture](../evidence/licenses/server-smoke-NOTICE.md).

All temporary test containers, Compose model volume and credentials were
removed. Temporary CDI specifications and hooks were removed after GPU tests;
the Docker daemon and NVIDIA driver were kept running throughout.

## Acceptance limits

The GitHub-hosted workflow has been prepared and its commands exercised locally;
a GitHub Actions run, GHCR publication and permanent deployment are separate
steps. Owner production acceptance remains pending. Profile B readiness verifies
GigaAM initialization; Russian recognition quality and simultaneous LLM memory
budgets need their own corpus and workload checks. Extended doctor checks remain
SRV-09 work, as described in [the server guide](../server.md).
