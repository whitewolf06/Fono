use std::env;
use std::path::PathBuf;

fn main() {
    // NOTE: SIMD-флаги для whisper.cpp задаются в `.cargo/config.toml` через [env],
    // потому что build.rs применяется только к нашему crate, а whisper.cpp
    // собирается как зависимость whisper-rs-sys.

    tauri_build::build();

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        sync_sherpa_prebuilt_libraries();
        copy_sherpa_dlls();
        copy_stt_worker_files();
    }
}

/// `sherpa-onnx-sys` keeps its downloaded prebuilt libraries in Cargo's target
/// directory. Its linker directive is target-directory-relative, so a custom
/// `CARGO_TARGET_DIR` (used for an isolated release build or CI) needs the
/// already downloaded archive mirrored there before the final link step.
fn sync_sherpa_prebuilt_libraries() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let default_target_dir = manifest_dir.join("target");
    let target_dir = cargo_target_dir(&manifest_dir);

    if target_dir == default_target_dir {
        return;
    }

    let source_dir = default_target_dir.join("sherpa-onnx-prebuilt");
    let destination_dir = target_dir.join("sherpa-onnx-prebuilt");
    if !source_dir.is_dir() || destination_dir.is_dir() {
        return;
    }

    if let Err(error) = copy_directory(&source_dir, &destination_dir) {
        println!(
            "cargo:warning=failed to mirror sherpa-onnx prebuilt libraries {} -> {}: {error}",
            source_dir.display(),
            destination_dir.display()
        );
    }
}

fn cargo_target_dir(manifest_dir: &std::path::Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                manifest_dir.join(path)
            }
        })
        .unwrap_or_else(|| manifest_dir.join("target"))
}

fn copy_directory(
    source_dir: &std::path::Path,
    destination_dir: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(destination_dir)?;
    for entry in std::fs::read_dir(source_dir)? {
        let entry = entry?;
        let source = entry.path();
        let destination = destination_dir.join(entry.file_name());
        if source.is_dir() {
            copy_directory(&source, &destination)?;
        } else {
            std::fs::copy(source, destination)?;
        }
    }
    Ok(())
}

/// Copies the generated CUDA/Vulkan worker executables beside a directly run
/// development or release binary. Tauri's installer consumes the same source
/// folder through `bundle.resources`; this copy is for `target/<profile>/fono.exe`.
fn copy_stt_worker_files() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let profile = env::var("PROFILE").unwrap();
    let source_dir = manifest_dir.join("resources").join("stt-workers");
    let target_dir = cargo_target_dir(&manifest_dir)
        .join(profile)
        .join("resources")
        .join("stt-workers");

    println!("cargo:rerun-if-changed={}", source_dir.display());
    if !source_dir.is_dir() {
        println!(
            "cargo:warning=STT workers are not prepared yet: {}",
            source_dir.display()
        );
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&target_dir) {
        println!(
            "cargo:warning=failed to create {}: {error}",
            target_dir.display()
        );
        return;
    }

    for entry in std::fs::read_dir(&source_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let source = entry.path();
        if !matches!(
            source.extension().and_then(|value| value.to_str()),
            Some("exe" | "dll")
        ) {
            continue;
        }
        let destination = target_dir.join(entry.file_name());
        if let Err(error) = std::fs::copy(&source, &destination) {
            println!(
                "cargo:warning=failed to copy STT worker {} -> {}: {error}",
                source.display(),
                destination.display()
            );
        }
    }
}

/// Копирует runtime DLL от sherpa-onnx (shared build) в ресурсы бандла.
/// При использовании feature `sherpa-wake` crate `sherpa-onnx-sys` кладёт DLL
/// рядом с бинарником в `target/<profile>`. Tauri bundler не забирает их
/// автоматически, поэтому копируем в `resources/sherpa-onnx/`.
fn copy_sherpa_dlls() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let profile = env::var("PROFILE").unwrap();
    let profile_dir = cargo_target_dir(&manifest_dir).join(&profile);

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
