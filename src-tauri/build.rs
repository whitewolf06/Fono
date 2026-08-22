use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const REQUIRED_STT_WORKERS: &[&str] = &["fono-stt-cuda-worker.exe", "fono-stt-vulkan-worker.exe"];

const REQUIRED_CUDA_RUNTIME_PREFIXES: &[&str] = &["cublas64_", "cublasLt64_", "cudart64_"];

const SHERPA_RUNTIME_DLLS: &[&str] = &[
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll",
];

fn main() {
    // SIMD flags for whisper.cpp are configured in .cargo/config.toml because
    // this build script only applies to the application crate.
    // The release-resource preparation build runs before the Sherpa DLLs
    // exist in `resources/sherpa-onnx`.  `tauri_build::build()` expands the
    // bundle resource glob eagerly, so it must be skipped for that isolated
    // pre-build; the subsequent Tauri bundle build runs it normally.
    let preparing_release_resources = env::var_os("FONO_PREPARING_RELEASE_RESOURCES").is_some();
    if !preparing_release_resources {
        tauri_build::build();
    }

    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SHERPA_WAKE");
    println!("cargo:rerun-if-env-changed=FONO_PREPARING_RELEASE_RESOURCES");
    println!("cargo:rerun-if-env-changed=FONO_BUILD_REVISION");
    println!("cargo:rerun-if-changed=../.git/HEAD");

    let revision = env::var("FONO_BUILD_REVISION").unwrap_or_else(|_| git_revision());
    println!("cargo:rustc-env=FONO_BUILD_REVISION={revision}");
    println!(
        "cargo:rustc-env=FONO_BUILD_PROFILE={}",
        env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string())
    );

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let layout = BuildLayout::from_env().unwrap_or_else(|error| {
        panic!("cannot determine Cargo output layout: {error}");
    });

    stage_stt_workers(&layout);

    if layout.is_release()
        && env::var_os("CARGO_FEATURE_SHERPA_WAKE").is_some()
        && !preparing_release_resources
    {
        validate_sherpa_runtime_manifest(&layout);
    }
}

fn git_revision() -> String {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let revision = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(&manifest_dir)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|revision| revision.trim().to_string())
        .filter(|revision| !revision.is_empty());
    let Some(revision) = revision else {
        return "unknown".to_string();
    };
    let is_dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(manifest_dir)
        .output()
        .ok()
        .is_some_and(|output| output.status.success() && !output.stdout.is_empty());

    if is_dirty {
        format!("{revision}-dirty")
    } else {
        revision
    }
}

#[derive(Debug)]
struct BuildLayout {
    manifest_dir: PathBuf,
    profile: String,
    profile_dir: PathBuf,
}

impl BuildLayout {
    fn from_env() -> Result<Self, String> {
        let manifest_dir = required_absolute_path("CARGO_MANIFEST_DIR")?;
        let out_dir = required_absolute_path("OUT_DIR")?;
        let profile =
            env::var("PROFILE").map_err(|error| format!("PROFILE is not set: {error}"))?;

        // Cargo does not expose a CLI --target-dir through CARGO_TARGET_DIR.
        // OUT_DIR is authoritative and has the shape:
        // <target-root>[/<target-triple>]/<profile>/build/<package-hash>/out.
        let package_build_dir = out_dir
            .parent()
            .ok_or_else(|| format!("OUT_DIR has no package directory: {}", out_dir.display()))?;
        let build_dir = package_build_dir
            .parent()
            .ok_or_else(|| format!("OUT_DIR has no build directory: {}", out_dir.display()))?;
        if build_dir.file_name() != Some(OsStr::new("build")) {
            return Err(format!(
                "unexpected OUT_DIR layout, expected a build directory: {}",
                out_dir.display()
            ));
        }
        let profile_dir = build_dir
            .parent()
            .ok_or_else(|| format!("OUT_DIR has no profile directory: {}", out_dir.display()))?
            .to_path_buf();
        if profile_dir.file_name() != Some(OsStr::new(&profile)) {
            return Err(format!(
                "OUT_DIR profile directory {} does not match PROFILE={profile}",
                profile_dir.display()
            ));
        }

        Ok(Self {
            manifest_dir,
            profile,
            profile_dir,
        })
    }

    fn is_release(&self) -> bool {
        self.profile == "release"
    }
}

fn required_absolute_path(name: &str) -> Result<PathBuf, String> {
    let value = env::var_os(name).ok_or_else(|| format!("{name} is not set"))?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{name} must be absolute, got {}", path.display()));
    }
    Ok(path)
}

/// Copies prepared GPU workers next to a directly run application executable.
///
/// A release build is a packaging boundary: missing or partial workers are a
/// hard error. Debug/test builds can run with the in-process CPU backend, so an
/// absent worker directory is reported but does not block normal development.
fn stage_stt_workers(layout: &BuildLayout) {
    let source_dir = layout.manifest_dir.join("resources").join("stt-workers");
    let destination_dir = layout.profile_dir.join("resources").join("stt-workers");

    println!("cargo:rerun-if-changed={}", source_dir.display());

    if !source_dir.is_dir() {
        build_problem(
            layout.is_release(),
            format!(
                "STT workers are not prepared at {}; run npm run build:workers before a release build",
                source_dir.display()
            ),
        );
        return;
    }

    let files = runtime_files(&source_dir).unwrap_or_else(|error| {
        build_problem(
            layout.is_release(),
            format!(
                "cannot inspect STT worker directory {}: {error}",
                source_dir.display()
            ),
        );
        Vec::new()
    });

    if layout.is_release() {
        validate_release_worker_manifest(&source_dir, &files);
    }

    if files.is_empty() {
        build_problem(
            layout.is_release(),
            format!("STT worker directory is empty: {}", source_dir.display()),
        );
        return;
    }

    if let Err(error) = fs::create_dir_all(&destination_dir) {
        build_problem(
            layout.is_release(),
            format!(
                "cannot create STT worker destination {}: {error}",
                destination_dir.display()
            ),
        );
        return;
    }

    for source in files {
        let destination = destination_dir.join(
            source
                .file_name()
                .expect("runtime_files only returns paths with a file name"),
        );
        if let Err(error) = fs::copy(&source, &destination) {
            build_problem(
                layout.is_release(),
                format!(
                    "cannot copy STT worker {} to {}: {error}",
                    source.display(),
                    destination.display()
                ),
            );
        }
    }
}

fn runtime_files(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let extension = path.extension().and_then(OsStr::to_str);
        if path.is_file() && matches!(extension, Some("exe" | "dll")) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn validate_release_worker_manifest(source_dir: &Path, files: &[PathBuf]) {
    let names: Vec<&str> = files
        .iter()
        .filter_map(|path| path.file_name().and_then(OsStr::to_str))
        .collect();

    let mut missing = Vec::new();
    for required in REQUIRED_STT_WORKERS {
        if !names.contains(required) {
            missing.push((*required).to_string());
        }
    }
    for prefix in REQUIRED_CUDA_RUNTIME_PREFIXES {
        if !names.iter().any(|name| name.starts_with(prefix)) {
            missing.push(format!("{prefix}*.dll"));
        }
    }

    let unexpected: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| {
            !REQUIRED_STT_WORKERS.contains(name)
                && !REQUIRED_CUDA_RUNTIME_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
        })
        .collect();

    if !missing.is_empty() {
        panic!(
            "incomplete release STT worker manifest in {}: missing {}",
            source_dir.display(),
            missing.join(", ")
        );
    }
    if !unexpected.is_empty() {
        panic!(
            "release STT worker manifest in {} contains unexpected runtime files: {}",
            source_dir.display(),
            unexpected.join(", ")
        );
    }
}

/// The release-preparation script owns source-tree staging. `build.rs` only
/// validates its deterministic result and never writes bundle resources.
fn validate_sherpa_runtime_manifest(layout: &BuildLayout) {
    let directory = layout.manifest_dir.join("resources").join("sherpa-onnx");
    let files = runtime_files(&directory).unwrap_or_else(|error| {
        panic!(
            "cannot inspect Sherpa runtime manifest in {}: {error}",
            directory.display()
        )
    });
    let names: Vec<&str> = files
        .iter()
        .filter_map(|path| path.file_name().and_then(OsStr::to_str))
        .collect();
    let missing: Vec<&str> = SHERPA_RUNTIME_DLLS
        .iter()
        .copied()
        .filter(|required| !names.contains(required))
        .collect();
    let unexpected: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| !SHERPA_RUNTIME_DLLS.contains(name))
        .collect();

    if !missing.is_empty() || !unexpected.is_empty() {
        panic!(
            "invalid Sherpa runtime manifest in {}: missing [{}], unexpected [{}]; run npm run prepare:release-resources",
            directory.display(),
            missing.join(", "),
            unexpected.join(", ")
        );
    }
}

fn build_problem(fatal: bool, message: String) {
    if fatal {
        panic!("{message}");
    }
    println!("cargo:warning={message}");
}
