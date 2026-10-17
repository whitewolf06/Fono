use std::fs;
use std::path::{Path, PathBuf};

const REQUIRED: &str = "fono-stt-vulkan-worker.exe";

pub fn files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if !kind.is_file() || kind.is_symlink() {
            return Err("base worker staging accepts regular files only".into());
        }
        paths.push(entry.path());
    }
    let names = paths
        .iter()
        .map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or("base worker filename is not valid UTF-8")
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_names(&names)?;
    Ok(paths)
}

fn validate_names(names: &[&str]) -> Result<(), String> {
    if names != [REQUIRED] {
        return Err(format!(
            "base worker staging must contain exactly {REQUIRED}; found {}",
            names.join(", ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_names;

    #[test]
    fn accepts_only_the_single_exact_vulkan_name() {
        assert!(validate_names(&["fono-stt-vulkan-worker.exe"]).is_ok());
    }

    #[test]
    fn rejects_missing_worker_or_cuda_instead_of_vulkan() {
        assert!(validate_names(&[]).is_err());
        assert!(validate_names(&["fono-stt-cuda-worker.exe"]).is_err());
    }

    #[test]
    fn rejects_stale_cuda_and_unknown_payloads_alongside_vulkan() {
        for stale in [
            "fono-stt-cuda-worker.exe",
            "cublas64_13.dll",
            "cublasLt64_13.dll",
            "cudart64_13.dll",
            "unexpected.dll",
            "unexpected.txt",
        ] {
            assert!(validate_names(&["fono-stt-vulkan-worker.exe", stale]).is_err());
        }
    }

    #[test]
    fn rejects_paths_case_aliases_and_duplicates() {
        for invalid in [
            "../fono-stt-vulkan-worker.exe",
            "FONO-STT-VULKAN-WORKER.EXE",
            "fono-stt-vulkan-worker.exe:stream",
        ] {
            assert!(validate_names(&[invalid]).is_err());
        }
        assert!(
            validate_names(&["fono-stt-vulkan-worker.exe", "fono-stt-vulkan-worker.exe"]).is_err()
        );
    }
}
