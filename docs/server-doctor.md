# Fono Server doctor

Run doctor before starting the server, with the service's user, configuration,
environment and mounts:

```sh
fono-server doctor --config /etc/fono/server.toml
fono-server doctor --config /etc/fono/server.toml --self-test-timeout-seconds 300
fono-server doctor --config /etc/fono/server.toml --skip-self-test
```

The default command performs a native recognition probe using the configured
default model and backend. It loads model weights and requires the corresponding
RAM/VRAM. It never captures a microphone, downloads weights, calls an LLM or
saves a transcript. The [embedded English/Russian fixtures](evidence/licenses/server-doctor-NOTICE.md)
and their upstream notices are shipped with the server.

## Report and exit status

A valid configuration produces one JSON report. Exit code 0 means the requested
required checks passed; code 1 means at least one failed. Invalid TOML or an
invalid configuration is rejected before inspection with a sanitized CLI error.

- `product_version` identifies the product build.
- `checks` contains stable check names, `required`, `ok`, a technical summary,
  and `remediation` for every failed check.
- `models` identifies required and optional managed model IDs and their disk
  verification state.
- `devices` and `workers` contain observed NVIDIA inventory and protocol checks.
- `storage` contains measured available/total bytes for model and temporary
  storage. Unknown values are omitted, rather than reported as zero.
- `self_test` has status `passed`, `failed` or `skipped`. Successful probes
  identify the managed model, backend, fixture duration and elapsed time.

Top-level `ready` depends on required checks. Optional warnings include a model
directory that is read-only, unused unavailable CUDA support and non-loopback
plain HTTP. `--skip-self-test` explicitly leaves the recognition check optional
and reports `self_test.status = "skipped"`; it never claims that speech passed.

The report excludes bearer secrets, credential names, PEM contents, transcripts,
configured addresses, file paths and raw native errors. NVIDIA device names and
driver versions are included as hardware inventory.

## Checks

| Area                 | What is checked                                                                                  | Typical action                                              |
| -------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------- |
| Startup model policy | Default model stays resident; required models fit `max_loaded`                                   | Correct default/preload placement and capacity              |
| Credentials          | Store format, current owner and mode 0600; at least one token                                    | Use the token CLI through a writable one-off mount          |
| Models               | Required artifacts exist and match pinned hashes; invalid optional artifacts are warnings        | Run `models verify` or repair with `models pull <id>`       |
| Directories          | Real directory ancestors and current-user access; temporary write/read/remove probe              | Set the intended service ownership/mounts                   |
| Model management     | Temporary write access to `models.dir`                                                           | Provide a writable model volume when using the model CLI    |
| Free space           | Actual filesystem availability; temporary capacity for `max_upload_mb` plus 1 MiB                | Increase tmpfs/disk capacity or lower the upload limit      |
| Listener             | A transient bind succeeds on the configured address and port                                     | Release a conflicting port or select an assigned interface  |
| Network              | Explicit private-network/host policy; warning for non-loopback HTTP                              | Restrict publication/firewall and use a tunnel or TLS proxy |
| TLS files            | Bounded regular PEM files, leaf validity dates, key permissions and matching public key          | Renew/repair the certificate and matching key               |
| NVIDIA               | Driver inventory through the NVML-backed `nvidia-smi`; requested GPU ordinals exist              | Check the driver and CDI mapping                            |
| Worker               | Fixed installed CUDA worker, safe layout and protocol-4 handshake for configured CUDA selections | Use matching binaries and expose driver libraries           |
| Native self-test     | Default model recognizes embedded speech, expected word fragments and selected backend agree     | Check weights, backend availability and memory              |

Temporary permission probes use unique files with private permissions and
remove only those files. Credentials and model weights are not rewritten. The
model-space recommendation is 64 MiB for management overhead; installed weights
can remain on read-only storage. Temporary-space sufficiency is required for
the configured upload limit.

TLS inspection accepts a matching unencrypted PEM key with restricted ownership
and mode 0600, 0400 or a trusted read-only group such as 0640. Encrypted keys fail
without an interactive password prompt. Certificate parsing/date/key checks do
not establish client trust. `tls_runtime` validates the pair with the same
rustls loader used at startup and renewal. See [native TLS setup](server-tls.md)
and the [network contract](server-security.md).

## Running services and deadlines

The bind test releases its socket immediately. A service already holding the
port causes `listener` to fail and the speech probe to be skipped. The existing
listener remains running; use its authenticated `/v1/health` for live status.
Doctor does not stop an existing service or change its configuration.

Failed required preflight checks skip model loading. Otherwise recognition runs
in a private child process. The deadline is 120 seconds by default, adjustable
from 1 to 600 seconds with `--self-test-timeout-seconds`. It covers model loading,
recognition and native cleanup. On Ubuntu, a timeout terminates the entire probe
process group, including its CUDA worker. Native stderr and unexpected child
output are discarded instead of being inserted into the report.

This is an installation preflight snapshot. Corpus quality, throughput under
concurrency and memory budgets with an LLM require their own workload checks.
See the [Ubuntu acceptance record](qa/server-doctor.md) for the tested build,
CPU/CUDA probes and negative scenarios.
