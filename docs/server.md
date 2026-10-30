# Fono Server on Ubuntu

Fono Server exposes [API v2](server-api.md) without Tauri, a microphone or a
desktop session. The CUDA image runs Whisper in a supervised child process and
keeps CPU Whisper and Sherpa offline recognition in the server process.

## Requirements

- Linux x86_64, Ubuntu 22.04 or 24.04, Docker Engine 28.3 or newer and Compose v2
  or newer with CDI device support.
- An NVIDIA GPU with compute capability 7.5 or newer. The image contains CUDA
  12.8.1, with kernels for SM 75, 80, 86, 89, 90, 100 and 120; RTX 3060 Ti is SM 86.
- NVIDIA Linux driver **570.124.06 or newer**, the matching full CUDA 12.8 Update 1
  driver baseline. See [NVIDIA's release notes](https://docs.nvidia.com/cuda/archive/12.8.1/cuda-toolkit-release-notes/index.html).
- NVIDIA Container Toolkit or its `nvidia-container-toolkit-base` package and a
  generated CDI specification. Follow [NVIDIA's CDI instructions](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/cdi-support.html).
  Docker supports `--device=nvidia.com/gpu=all` through
  [native CDI support](https://docs.docker.com/reference/cli/docker/container/run/#cdi-devices).

Check the host and container before building Fono:

```sh
nvidia-smi
nvidia-ctk cdi list
docker run --rm --device=nvidia.com/gpu=all \
  nvidia/cuda:12.8.1-runtime-ubuntu22.04 nvidia-smi -L
```

Updating a host driver or restarting Docker affects other workloads and is a
separate maintenance operation. A CUDA compiler is only needed inside the build
stage. The host does not need the CUDA toolkit.

## Build and configure

From the repository root:

```sh
docker build -f packaging/docker/Dockerfile.cuda -t fono-server:cuda-local .
cd packaging/docker
cp server.profile-a.toml server.toml
install -d -m 700 credentials
sudo chown 10001:10001 credentials
docker compose -f compose.example.yml config --quiet
```

The image has a fixed UID/GID of **10001:10001**, an API healthcheck and a fixed
worker path next to the server in `/usr/lib/fono`. Sherpa and Silero downloads
are checked against pinned SHA-256 digests. The build invokes Cargo separately
for the server and CUDA worker, preventing CUDA libraries from being linked
into the server. CUDA build and runtime bases and the Rust toolchain image are
pinned by digest.

Build once before the initialization commands below, or pass `--build` to the
first Compose run. This build includes native server contract tests and can
take considerably longer than starting the prepared image.

```sh
docker compose -f compose.example.yml build
```

The default port publication is `127.0.0.1:17832`. For a protected LAN or private
tunnel, add the exact server IP or DNS name to `server.toml`'s `allowed_hosts`,
then set `FONO_BIND_IP` to the private interface address:

```sh
export FONO_BIND_IP=192.168.1.8
```

The container listens on `0.0.0.0` so Docker port forwarding works. The host
publication controls which interface is reachable. Plain HTTP is intended for
a protected private network; use a TLS reverse proxy or encrypted tunnel when
transport protection is required. Native `[tls]` is currently rejected. See
[the network contract](server-security.md).

## Initialize models and credentials

The model volume is populated with the image directory's UID/GID on first use.
Download the default model and create a credential with a one-off container:

```sh
docker compose -f compose.example.yml run --rm fono models pull whisper:large_turbo
docker compose -f compose.example.yml run --rm \
  --volume "$PWD/credentials:/etc/fono/credentials:rw" \
  fono token create main-pc
```

The token is printed once. Store it in the client credential store. The server
persists salted digests only. The token file must be mode `0600` and owned by
UID 10001. The Compose service mounts the **credential directory** read-only so
CLI atomic replacement and revocation become visible on the next request.
Rotate or revoke tokens with the same one-off writable mount. No secret belongs
in `server.toml`, Compose environment variables, the image or Git.

```sh
docker compose -f compose.example.yml run --rm fono doctor
docker compose -f compose.example.yml up -d --no-build
docker compose -f compose.example.yml ps
curl --fail http://127.0.0.1:17832/healthz
```

If the port is published on a LAN interface, use that IP in the curl URL.
`/healthz` returns 503 while the default model loads and 200 after it becomes
ready. All detailed API routes require a bearer token.

`doctor` is a full preflight check: credentials, model hashes, directories and
free space, listener availability, network exposure, TLS files, driver inventory,
worker handshakes and recognition of embedded public speech on the default
model. Run it before starting the service, as the same service account. It
prints JSON with remediation hints and returns nonzero for failed required
checks. See [the doctor guide](server-doctor.md) for checks, timeouts and
`--skip-self-test`. Native TLS serving remains SRV-11 work; valid TLS files do
not enable an unsupported listener.

The service is non-root, has a read-only root filesystem and credentials,
drops capabilities, and uses a bounded temporary filesystem. Only the model
volume is persistent and writable. `restart: unless-stopped` applies after the
operator starts the service. Stop it with `docker compose -f compose.example.yml down`;
the model volume and credentials are retained.

## Profiles and device placement

Profile A keeps `whisper:large_turbo` resident on `cuda:0`. GigaAM v3 is an
optional second CPU recognizer; install it before requesting its model ID:

```sh
docker compose -f compose.example.yml run --rm fono models pull giga_am:v3_e2e_rnnt_int8
```

Profile B uses resident GigaAM on CPU as the default and lets CUDA Whisper unload
after 60 seconds idle. This keeps readiness stable while freeing speech GPU
weights for a separately managed LLM. Copy `server.profile-b.toml` to
`server.toml`, pull **both** models and restart the Fono service. Request
`model=whisper:large_turbo` explicitly to use GPU recognition with this profile.
The LLM is configured and started separately. An 8 GB GPU does not guarantee
that Whisper and a 7–8B LLM fit simultaneously; measure actual free memory and
choose the model size and context accordingly.

`device = "cuda:N"` selects an observed NVIDIA ordinal. Only that worker receives
`CUDA_DEVICE_ORDER=PCI_BUS_ID` and `CUDA_VISIBLE_DEVICES=N`; server and sibling
workers keep their own environment. CPU and each GPU have separate inference
lanes. Offline Sherpa models support CPU placement only. No CPU fallback is
configured for an explicit CUDA placement. A failed GPU worker produces
`502 worker_failed`; the next request reloads its model in a new worker.

## Verification and publication

Structured JSON logs and protected queue, timing, RTF and driver metrics are
described in [the observability guide](server-observability.md).

Run the reproducible CPU smoke against the CUDA image without GPU access:

```sh
python3 scripts/smoke-server-container.py --image fono-server:cuda-local
```

Run this command from the repository root on Linux. It creates temporary
credentials and model storage, checks authenticated CPU recognition using a
[pinned public sample](evidence/licenses/server-smoke-NOTICE.md), and removes its
container and temporary files. No host service or microphone is used.

[The container workflow](../.github/workflows/server-container.yml) builds on
GitHub-hosted runners and executes the CPU smoke. A pushed release tag builds
and tests only. Publication requires a manual `workflow_dispatch` on the exact
`v<package.json version>` tag, `publish=true`, and the matching full commit SHA.
Configure required reviewers for the `fono-server-release` GitHub environment.
Only that separate publication job receives `packages: write`; it publishes
the tested artifact as `:<version>-cuda` and `:cuda`. Creating the workflow or a
local image does not publish to GHCR. Image signing and SBOM are subsequent work.

For GPU acceptance, inspect `doctor`, capabilities, a real GPU recognition
request, the worker process in `nvidia-smi`, and worker crash/recovery. CPU CI
smoke proves API and packaging compatibility; it does not provide GPU evidence.

On an isolated GPU test host, the same smoke can exercise the worker failure
contract. It terminates only the GPU worker in its own temporary container:

```sh
python3 scripts/smoke-server-container.py --image fono-server:cuda-local \
  --device cuda:0 --crash-worker
```

The [recorded Ubuntu/CUDA acceptance](qa/server-cuda.md) identifies the tested
candidate, artifacts and limits of the checks.
