# TLS and request-rate acceptance — 2026-10-30

Candidate: Fono **0.8.29**, API 2, worker protocol 4. Native inputs are based on
`0d3a288` plus SRV-11. Host: Ubuntu 24.04.5, RTX 3060 Ti, driver 595.84.
Container build/runtime use Ubuntu 22.04 and CUDA 12.8.1.

## Artifact identity

- Source archive SHA-256: `55c976e041020360f30617d3ecf6a96157cf130da943044bd4c0f6e81d8a431c`.
- Local image ID: `sha256:aafe7e2babbc7d57cea8aba6b356514010447f899c23450ccfedb24a59dda436`.
- Server SHA-256: `bc5e5e012c67886792a0df6b59d97c163e2a53717b8744d3274dc338d3d885cc`.
- CUDA worker SHA-256: `c804378a65d5cf10c1cc82e4174090b7d09dafcd0a6bdadb90826afef8ba02c0`.

The local build used Docker's bundled frontend, as in the preceding
[observability pass](server-observability.md). A temporary Dockerfile copy
omitted only the external `# syntax` directive. Pinned bases, native sources
and `build-cuda.sh` commands were unchanged. The repository Dockerfile was not
modified and the temporary copy was removed. No GitHub Actions run or image
publication is claimed by this local build.

## Results

| Check                                | Observed result                                                                                                                               |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Native contracts and strict Clippy   | 270 tests passed; 3 existing external-fixture tests ignored; server/shared crates and release worker Clippy passed                            |
| HTTPS on CPU and CUDA                | Authenticated native recognition and OpenAI multipart transcription returned 200 on the configured device                                     |
| Doctor with TLS configured           | Required checks and embedded recognition passed; `tls_runtime` used the real rustls loader                                                    |
| TLS HTTP/2                           | Verified HTTPS health request negotiated HTTP/2 on CPU and CUDA                                                                               |
| HTTPS container healthcheck          | CA-verified, name-verified check reported `healthy` for all three test containers                                                             |
| Client trust and hostname            | An untrusted CA and a wrong certificate hostname were rejected by the client                                                                  |
| Plaintext on HTTPS listener          | No HTTP response was accepted; TLS remained mandatory                                                                                         |
| Bad startup material                 | Invalid certificate failed container startup with code 1 and safe diagnostics                                                                 |
| Invalid replacement plus real SIGHUP | `tls_reload_rejected`; new clients still received the previous certificate                                                                    |
| Valid replacement plus real SIGHUP   | New clients observed a changed certificate; the existing TLS connection continued successfully                                                |
| Active request during reload         | Deterministic Rust test held an HTTP handler open across a rejected and accepted reload; it completed and its connection remained usable      |
| Certificate/key validation           | Unit tests rejected mismatched keys, expired certificates, unsafe permissions, encrypted keys and missing material                            |
| Rate rejection before body           | Exhausted token received 429 OpenAI `rate_limited` with `Retry-After` and request ID after headers only; a declared 10 MB body was never sent |
| Quota identity, refill and liveness  | Another token retained its quota; invalid credentials returned 401; refill permitted a later call; anonymous `/healthz` remained available    |
| Quota survives TLS renewal           | SIGHUP did not reset an exhausted credential's bucket                                                                                         |
| Token-bucket unit boundaries         | Exact refill and rounding, concurrent atomic budget and bounded-state pressure passed                                                         |
| TLS shutdown                         | CPU, limited CPU and CUDA containers exited with code 0 within five seconds, including an unfinished handshake                                |
| Technical-log privacy                | JSON logs contained none of the test tokens, private markers, PEM, configured TLS paths or transcription text                                 |
| Compose TLS override                 | Merged configuration validated; healthcheck used HTTPS with an explicit CA/name                                                               |
| HTTP compatibility smoke             | Fresh temporary model storage; doctor passed; health/capabilities/recognize 200 and anonymous discovery 401                                   |
| Frontend/release regressions         | Typecheck, lint, build, 412 V3 tests, 52 release tests and 2 publication-policy tests passed                                                  |

The live quota test used 6 requests/minute with burst 2 to verify a real refill
without relying on subsecond timing. Unit tests used explicit monotonic times
to verify exact boundaries. Production defaults remain 120 requests/minute and
burst 20; inference concurrency limits are independent.

Tests used an ephemeral private CA, its signed localhost/IP certificates and
[pinned public audio](../evidence/licenses/server-smoke-NOTICE.md). Trust was
scoped to test clients and the container healthcheck; no OS trust store was
changed. The live server used image UID 10001, read-only mounts and dropped
capabilities. All temporary containers, credentials, keys, CA material and CDI
hooks/specifications were removed. Existing host services were not restarted.

Rustls was updated to 0.23.46 and webpki to 0.103.16. License-file SHA-256 checks
confirmed that the TLS libraries' complete notice texts were already included
in the shipped aggregate notice bundle.

## Limits and operation

This verifies the protected native TLS scenario and per-credential quotas.
Certificate issuance for an actual domain/tailnet, renewal scheduling, permanent
deployment, long-running load/soak behavior and owner production acceptance
remain separate steps. Tailscale issuance was documented but not performed.

See [TLS and renewal](../server-tls.md), the
[network/rate contract](../server-security.md) and
[observability](../server-observability.md) for configuration and operational
limits. SIGHUP changes TLS material only; other configuration changes require
an explicit restart.
