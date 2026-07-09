use std::env;
use std::path::PathBuf;

fn main() {
    // NOTE: SIMD-флаги для whisper.cpp задаются в `.cargo/config.toml` через [env],
    // потому что build.rs применяется только к нашему crate, а whisper.cpp
    // собирается как зависимость whisper-rs-sys.

    tauri_build::build();

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        copy_sherpa_dlls();
    }
}

/// Копирует runtime DLL от sherpa-onnx (shared build) в ресурсы бандла.
/// При использовании feature `sherpa-wake` crate `sherpa-onnx-sys` кладёт DLL
/// рядом с бинарником в `target/<profile>`. Tauri bundler не забирает их
/// автоматически, поэтому копируем в `resources/sherpa-onnx/`.
fn copy_sherpa_dlls() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let profile = env::var("PROFILE").unwrap();
    let profile_dir = manifest_dir.join("target").join(&profile);

    let out_dir = manifest_dir.join("resources").join("sherpa-onnx");
    println!(
        "cargo:warning=copy_sherpa_dlls: profile_dir={} out_dir={}",
        profile_dir.display(),
        out_dir.display()
    );
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        println!("cargo:warning=failed to create {}: {e}", out_dir.display());
        return;
    }

    let entries = match std::fs::read_dir(&profile_dir) {
        Ok(e) => e,
        Err(e) => {
            println!(
                "cargo:warning=failed to read {}: {e}",
                profile_dir.display()
            );
            return;
        }
    };

    let mut copied = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("dll") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        // Не копируем собственные DLL приложения.
        if name.starts_with("fono") || name.starts_with("whisperclone") {
            continue;
        }
        let dest = out_dir.join(name);
        if let Err(e) = std::fs::copy(&path, &dest) {
            println!(
                "cargo:warning=failed to copy {} -> {}: {e}",
                path.display(),
                dest.display()
            );
        } else {
            copied += 1;
            println!("cargo:warning=copied {name}");
        }
    }

    println!("cargo:warning=copied {copied} sherpa-onnx runtime DLLs");
}
