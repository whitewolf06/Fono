# Extended doctor acceptance — 2026-10-29

Candidate: Fono **0.8.27**, API 2, worker protocol 4. Source snapshot based on
`13fe4cf` plus the SRV-09 implementation. Host: Ubuntu 24.04.5, RTX 3060 Ti
with 8192 MiB and NVIDIA driver 595.84. The container uses Ubuntu 22.04 and
CUDA 12.8.1.

## Artifact identity

- Source archive SHA-256: `28116f368fa51cfbea0a88020e4c8dfeaa7436d06cd30409b03d0a7143dbe2d9`.
- Local image ID: `sha256:f18b603acf549c29ee1cc9d5c7a7fe2d5583b453e46ccbc9a38c33389accfa96`.
- Server SHA-256: `f3393c331d96f68b404495d7e08a40d2e8c76a6e467d98d5ac56f0117b7d840e`.
- CUDA worker SHA-256: `c804378a65d5cf10c1cc82e4174090b7d09dafcd0a6bdadb90826afef8ba02c0`.

The source archive contains the native build inputs. QA scripts and this
acceptance record do not enter the Docker build context. The image is a local
test artifact; it has not been published.

## Results

| Check                                   | Observed result                                                                                                                        |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Rust server/shared tests                | 255 passed; 3 existing tests requiring external fixtures ignored                                                                       |
| Strict Clippy                           | Server, STT, service, types and release CUDA worker passed                                                                             |
| Whisper tiny on CPU                     | Default doctor recognized embedded English speech; `ready: true`                                                                       |
| GigaAM v3 on CPU                        | Embedded Russian speech passed; fixture duration 11290 ms                                                                              |
| Whisper tiny and large turbo on CUDA    | Embedded English speech and configured `cuda:0` backend passed                                                                         |
| Default service identity                | CUDA checks passed as UID 10001 with read-only root, models, configuration and credentials                                             |
| GPU not exposed to a CUDA configuration | Required driver check failed; native recognition skipped                                                                               |
| Read-only model store                   | Serving readiness passed; model-management write access reported as an optional warning                                                |
| Insufficient temporary space            | Upload limit of 1024 MiB with a 128 MiB tmpfs failed the required space check                                                          |
| Unwritable temporary directory          | Required permission check failed before recognition                                                                                    |
| Unsafe token permissions                | Mode 0644 failed; returning the test token to 0600 restored access                                                                     |
| Valid TLS certificate/key               | PEM, validity, key match and permissions passed; unsupported native TLS listener remained a required failure                           |
| Invalid TLS inputs                      | Unit checks rejected expired certificates, mismatched keys, unsafe permissions, encrypted keys and non-regular files without prompting |
| Occupied port                           | Doctor failed the bind check and skipped recognition; the existing isolated service still returned `/healthz` 200                      |
| Explicit `--skip-self-test`             | `self_test.status: skipped`; required preflight checks still governed readiness                                                        |
| Native deadline                         | A one-second deadline stopped the CPU turbo probe; Linux unit checks also verified termination of a child process group                |
| Redaction and remediation               | Reports omitted configured paths, hostnames and PEM data; failed checks contained remediation hints                                    |
| Frontend/release regression             | Typecheck, lint, build, 412 V3 tests and 52 release tests passed                                                                       |
| Publication policy                      | 2 tests passed, including mutations removing release gates                                                                             |

All speech used [pinned public fixtures](../evidence/licenses/server-doctor-NOTICE.md).
No microphone or personal recording was used. Permission tests operated only on
their own temporary credentials and files.

The CUDA one-second deadline also failed the probe as expected. After doctor
returned, the isolated container had no remaining live Fono server or worker
processes. Successful native probe timings were 556 ms for tiny and 2062 ms for
large turbo on this host; these observations are not throughput guarantees.

The exact CI smoke ran with fresh temporary model storage. `/healthz`,
authenticated capabilities and recognition returned 200; anonymous discovery
returned 401. Doctor reported the selected tiny CPU probe as `passed`.

All temporary test containers and credentials were removed. The temporary CDI
specification and hook were removed after GPU tests, and the driver reported no
remaining Fono compute process. No permanent service or Docker daemon restart
was needed.

## Acceptance limits

Doctor verifies installation readiness with the configured default model. This
does not establish corpus quality, sustained concurrency or simultaneous LLM
memory budgets. Native TLS serving and certificate reload remain SRV-11 work;
certificate inspection does not claim client trust or enable a TLS listener.

GitHub Actions execution, image publication, permanent deployment and owner
production acceptance remain separate steps. See the [doctor guide](../server-doctor.md)
for invocation, required checks and deadline behavior.
