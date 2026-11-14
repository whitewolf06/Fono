#!/usr/bin/env bash
set -euo pipefail
mkdir -p /out
export GITHUB_ENV=/tmp/fono-vulkan-build.env
export GGML_NATIVE=OFF GGML_AVX=OFF GGML_AVX2=OFF GGML_FMA=OFF GGML_F16C=OFF GGML_AVX512=OFF
bash scripts/bootstrap-ci-linux.sh
source "$GITHUB_ENV"
export SHERPA_ONNX_ARCHIVE_DIR
find apps crates -type f -exec touch {} +
touch Cargo.toml Cargo.lock package.json

# The host server stays CPU-linked. GPU code lives only in its own worker.
cargo build --locked --release -p fono-server --no-default-features --features vulkan-worker
install -Dm755 src-tauri/target/release/fono-server /out/fono-server
cargo test --locked -p fono-server -p fono-stt -p fono-service -p fono-types \
    --no-default-features --features fono-server/vulkan-worker --all-targets
cargo clippy --locked -p fono-server -p fono-stt -p fono-service -p fono-types \
    --no-default-features --features fono-server/vulkan-worker --all-targets -- -D warnings
cmake -S crates/fono-stt-vulkan-worker -B /tmp/fono-vulkan-build \
    -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=OFF -DGGML_AVX=OFF -DGGML_AVX2=OFF \
    -DGGML_FMA=OFF -DGGML_F16C=OFF -DGGML_AVX512=OFF
cmake --build /tmp/fono-vulkan-build --target fono-stt-vulkan-worker --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-4}"
install -Dm755 /tmp/fono-vulkan-build/fono-stt-vulkan-worker /out/fono-stt-vulkan-worker
python3 packaging/docker/copy-runtime.py
