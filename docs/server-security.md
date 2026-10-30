# Fono Server network security

The standalone Ubuntu server listens on `127.0.0.1:17832` by default. Native
rustls HTTPS is available through `[tls]`; see [TLS and renewal](server-tls.md).
Keep the listener on loopback unless access is
restricted by a private network, VPN, or tunnel and the host firewall.

To serve a protected private network, set all three fields in
`packaging/server.toml.example` before starting the server:

```toml
[server]
bind = "192.168.10.10:17832"
trusted_private_network = true
allowed_hosts = ["192.168.10.10", "fono.internal"]
```

`trusted_private_network` is an explicit operator assertion. The server rejects
a non-loopback bind unless it is `true` and `allowed_hosts` is explicitly set
to a nonempty list. Protect the listener at the network boundary as well. A
Host allowlist checks the HTTP authority; it is not authentication or a
firewall. The server emits a structured warning when it binds outside loopback
without TLS. Invalid TLS material fails startup before bind.

`allowed_hosts` contains exact DNS names or IP literals without ports. Matching
is case insensitive for DNS names; suffix matches and wildcards are rejected.
For loopback, the default list is `localhost`, `127.0.0.1`, and `::1`. Requests
with an invalid, missing, duplicate, or unlisted Host are rejected before
authentication or body extraction. Clients normally send Host automatically;
include the server's IP or DNS name in the allowlist if that is how clients
connect.

The server requires a bearer token for recognition and job routes. CORS is
disabled; cross-origin preflight responses do not grant browser access. The
`max_upload_mb` and `max_audio_seconds` limits still apply. The
`body_read_timeout` setting defaults to `60s` and bounds the total time spent
receiving a JSON or audio upload body, including multipart fields. Exceeding
that deadline returns HTTP 408 with `body_read_timeout`. The deadline does not
limit model loading, decoding, or transcription time.

These settings can also be supplied through `FONO_SERVER_BIND`,
`FONO_SERVER_TRUSTED_PRIVATE_NETWORK`, `FONO_SERVER_ALLOWED_HOSTS` (a TOML array
literal), and `FONO_SERVER_BODY_READ_TIMEOUT`. Environment overrides go through
the same validation as the TOML file.

## Per-credential request rate

```toml
[rate_limit]
enabled = true
requests_per_minute = 120
burst = 20
```

The default token bucket starts with 20 credits and refills at two requests per
second. This is a sustained average with a burst allowance, not a fixed-minute
counter. Settings are independent of `max_inflight_per_token` and queue/device
admission. Supported ranges are 1–60000 requests per minute and burst 1–1024.

Every authenticated route, including discovery, job polling/cancellation and
metrics, consumes one credit. `/healthz` is exempt. Invalid/revoked credentials
never allocate a bucket or consume another credential's credits. Buckets are
keyed by the persisted credential digest, shared across connections and scopes;
rotation creates a distinct credential. Scope checks remain enforced by routes.

An exhausted bucket returns 429 with `error.code=rate_limited`, a whole-second
`Retry-After` and `X-Request-Id`, before audio body extraction or inference
admission. The OpenAI route retains its error envelope and uses
`error.type=rate_limit_error`. Queue saturation retains its distinct `busy` code.

State is in memory, bounded to 4096 credential buckets. Capacity pressure
returns 429 rather than evicting an exhausted bucket; only fully recovered
entries can be removed. Restart resets budgets; SIGHUP certificate renewal does
not. Environment overrides are `FONO_RATE_LIMIT_ENABLED`,
`FONO_RATE_LIMIT_REQUESTS_PER_MINUTE` and `FONO_RATE_LIMIT_BURST`.
