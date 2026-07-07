fn main() {
    // NOTE: SIMD-флаги для whisper.cpp задаются в `.cargo/config.toml` через [env],
    // потому что build.rs применяется только к нашему crate, а whisper.cpp
    // собирается как зависимость whisper-rs-sys.

    tauri_build::build()
}
