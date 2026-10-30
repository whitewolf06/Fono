# Fono Server observability

## Technical JSON logs

`fono-server serve` writes one JSON object per event to stdout. Configure the
threshold through TOML or `FONO_LOGGING_LEVEL`:

```toml
[logging]
level = "info"
```

Accepted levels are `off`, `error`, `warn`, `info` and `debug`. The default is
`info`. Startup, listener, model readiness and shutdown are informational;
non-loopback exposure and HTTP 4xx are warnings; HTTP 5xx and failed native
windows are errors. Successful HTTP responses and native windows appear at
`debug`. CLI commands such as `doctor` retain their own stdout formats.

Events have a UTC `timestamp`, `level` and an `event` discriminator. Lifecycle
events carry a fixed `phase`; HTTP completion carries numeric `status` and
`elapsed_us`; inference windows carry a bounded `outcome`, `elapsed_us` and
`audio_ms`. No event accepts free-form error messages, transcripts, prompts,
glossaries, headers, URLs, query strings, request IDs, model paths or credentials.
Native/third-party tracing output is not subscribed into this stream and
`RUST_LOG` does not override the schema or configured threshold.
TLS renewal adds fixed lifecycle phases `tls_reloaded` and
`tls_reload_rejected`; certificate names, paths and parser errors remain absent.

The writer runs on its own thread with a 256-record queue. Producers never wait
for stdout I/O. Queue overflow and write failures increment
`fono_log_dropped_total`. Normal shutdown closes log admission and allows at
most one additional second to drain the writer; a blocked consumer cannot hold
the process open indefinitely. Logs are best effort, while metrics retain their
independent observations.

## Protected metrics

Enable metrics explicitly and use an admin credential:

```toml
[metrics]
enabled = true
```

```sh
curl --fail -H "Authorization: Bearer $FONO_ADMIN_TOKEN" \
  http://127.0.0.1:17832/metrics
```

All metrics use bounded labels. There are no per-client, per-token, per-request,
URL-derived or transcript-derived series. HTTP status values come from a fixed
list plus `other`; priorities and native outcomes are fixed enumerations.
Numeric GPU ordinals come from the observed driver inventory.

| Metric                                                                      | Meaning                                                                                                               |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `fono_http_requests_total`, `fono_http_errors_total`, `fono_http_inflight`  | Admitted HTTP calls, returned 4xx/5xx and active handler futures                                                      |
| `fono_http_responses_total{status}`                                         | Completed responses by bounded HTTP status                                                                            |
| `fono_http_request_duration_seconds`                                        | Handler completion duration, including body receipt and downstream work                                               |
| `fono_stt_queue_wait_seconds`                                               | Waiting for an actual inference device permit, including cancelled waits                                              |
| `fono_stt_waiting{priority}`, `fono_stt_active{priority}`                   | Threads waiting for device permits and active native windows                                                          |
| `fono_stt_decode_seconds`                                                   | Native window wall time for every completed attempt, including failed/preempted attempts                              |
| `fono_stt_windows_total{outcome}`                                           | `ok`, `preempted`, `cancelled`, `worker_failed`, or `failed` native attempts                                          |
| `fono_stt_realtime_factor`                                                  | Successful window wall time divided by its audio duration; excludes queueing/model acquisition and discarded attempts |
| `fono_model_acquire_seconds`                                                | Model acquisition attempts, including verification/loading, and startup preload attempts                              |
| `fono_audio_decode_seconds`                                                 | Native/OpenAI synchronous HTTP audio decoding attempts; legacy file jobs decode inside their shared service           |
| `fono_models{state}`                                                        | Pool reservations in loaded/loading/retiring states                                                                   |
| `fono_jobs{state}`, `fono_activity`                                         | Existing aggregate job states and actual admitted background activity                                                 |
| `fono_gpu_inventory_available`                                              | Last driver inventory succeeded; zero also covers a CPU-only build or no sample yet                                   |
| `fono_gpu_memory_total_bytes{device}`, `fono_gpu_memory_free_bytes{device}` | Observed whole-device memory; absent when inventory is unavailable                                                    |
| `fono_log_dropped_total`                                                    | Log queue overflow or writer failures                                                                                 |

Duration and RTF families are histograms with cumulative buckets, `_sum`,
`_count` and a `+Inf` bucket following the
[Prometheus text exposition contract](https://prometheus.io/docs/instrumenting/exposition_formats/#histograms-and-summaries).
The HTTP duration family previously exposed only a summary's sum/count. Those
sample names remain; its type is now `histogram` and bucket samples are added.

Handler futures dropped by a disconnected caller release the HTTP inflight
gauge without adding a completed response or duration. Native work that has
already started retains its existing activity/admission lease and records its
actual eventual completion. Preemption records an attempt and the retried
window records a separate attempt; only a successful retained window adds RTF.

CUDA builds poll the NVML-backed `nvidia-smi` inventory every five seconds on a
blocking worker, outside HTTP handling. A failed sample removes memory series
instead of inventing zero free memory. These are host GPU measurements, not
Fono worker allocations or a promise about a concurrent LLM's memory budget.

Metrics reset on process restart and are independent of log level. Scraping
`/metrics` itself is counted when its handler completes, after that scrape's
snapshot. No logging setting changes authentication, model placement, scheduler
priority or cancellation behavior.
See the [Ubuntu/CUDA acceptance record](qa/server-observability.md) for the
tested build, observed events and validation limits.
