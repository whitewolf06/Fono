# fono-service

Shared audio ingestion, bounded transcription jobs, transcript export and HTTP
routes. The crate depends on Fono's common types and STT engine, and has no
Tauri or desktop dependency.

The host supplies a `TranscriptionRuntime`, an owning `ActivityGuard`, a short
`CaptureGate` for atomic queue admission, and a `TokenVerifier`. Pending jobs
and actual native work retain activity leases through cancellation and cleanup.
The HTTP host binds its own listener and calls `rest_api::serve` or
`serve_with_shutdown`; it chooses the address, runtime and shutdown policy.

`jobs_routes()` and `meta_routes()` retain the existing local API v1 wire
contract. `recognize_routes()` and `openai_routes()` are extension points for
the separately planned API-02 and API-06 work. The desktop adapter keeps the
current loopback binding, updater fence, capture admission and UI events.

The included OpenAPI document describes the current local jobs API. API v2
routes, server token files and network exposure belong to later tasks.
