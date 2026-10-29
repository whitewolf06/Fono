#!/usr/bin/env bash
set -euo pipefail

export GITHUB_ENV=/tmp/fono-build-env
export CARGO_BUILD_JOBS="${BUILD_JOBS:-4}"
export CMAKE_BUILD_PARALLEL_LEVEL="$CARGO_BUILD_JOBS"
export CMAKE_CUDA_ARCHITECTURES="${CUDA_ARCHITECTURES:-75;80;86;89;90;100;120}"
export CMAKE_CUDA_COMPILER=/usr/local/cuda/bin/nvcc
# Generic CPU kernels allow the same image to run on older x86_64 machines.
export GGML_NATIVE=OFF GGML_AVX=OFF GGML_AVX2=OFF GGML_FMA=OFF GGML_F16C=OFF
export GGML_AVX512=OFF
bash scripts/bootstrap-ci-linux.sh
source "$GITHUB_ENV"
export SHERPA_ONNX_ARCHIVE_DIR

# COPY may preserve source mtimes older than this builder's cached Cargo
# invocation (notably when a Windows checkout is archived to a Linux host).
# Refresh only workspace inputs so Cargo cannot reuse stale application code.
# Registry/native dependencies keep their cache; no cargo clean is needed.
find apps crates -type f -exec touch {} +
touch Cargo.toml Cargo.lock package.json

# Separate invocations are intentional: Cargo must not unify CUDA into the server.
cargo build --locked --release -p fono-server --no-default-features --features cuda-worker
install -Dm755 src-tauri/target/release/fono-server /out/fono-server
# Run contract tests in the build environment, which needs no GPU.
cargo test --locked -p fono-server -p fono-stt -p fono-service -p fono-types \
  --no-default-features --features fono-server/cuda-worker --all-targets
cargo clippy --locked -p fono-server -p fono-stt -p fono-service -p fono-types \
  --no-default-features --features fono-server/cuda-worker --all-targets -- -D warnings
cargo build --locked --release -p fono-stt-worker --features cuda --bin fono-stt-worker
cargo clippy --locked --release -p fono-stt-worker --features cuda --bin fono-stt-worker -- -D warnings
install -Dm755 src-tauri/target/release/fono-stt-worker /out/fono-stt-cuda-worker

python3 - <<'PY'
import glob
import hashlib
import json
import os
import shutil
import tarfile

with open('scripts/ci-native-dependencies.json', encoding='utf-8') as source:
    spec = json.load(source)['sherpaLinux']
archive = glob.glob(os.path.join(os.environ['SHERPA_ONNX_ARCHIVE_DIR'], '*.tar.bz2'))
if len(archive) != 1:
    raise SystemExit('Expected one verified Sherpa archive')
with tarfile.open(archive[0], 'r:bz2') as source:
    source.extractall('/tmp/fono-runtime', filter='data')
for path in glob.glob('/tmp/fono-runtime/' + spec['directory'] + '/lib/*.so*'):
    # Materialize SONAME links; the final image is self-contained.
    shutil.copyfile(path, '/out/' + os.path.basename(path))
vad = 'src-tauri/resources/vad/silero_vad.onnx'
with open(vad, 'rb') as source:
    if hashlib.sha256(source.read()).hexdigest() != '9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6':
        raise SystemExit('Silero VAD SHA-256 mismatch')
shutil.copyfile(vad, '/out/silero_vad.onnx')
shutil.copyfile('LICENSE', '/out/LICENSE')
shutil.copyfile('docs/evidence/licenses/offline-stt-NOTICE.md', '/out/offline-stt-NOTICE.md')
shutil.copyfile('docs/evidence/licenses/server-doctor-NOTICE.md', '/out/server-doctor-NOTICE.md')
for path in glob.glob('apps/fono-server/resources/doctor-*-LICENSE.txt'):
    shutil.copyfile(path, '/out/' + os.path.basename(path))
shutil.copyfile('licenses/dependency-notices.txt', '/out/dependency-notices.txt')
for name in ('sherpa-onnx-1.13.4-APACHE-2.0.txt', 'onnxruntime-1.27.0-MIT.txt',
             'onnxruntime-1.27.0-ThirdPartyNotices.txt', 'silero-vad-MIT.txt'):
    shutil.copyfile('licenses/upstream/' + name, '/out/' + name)
PY
