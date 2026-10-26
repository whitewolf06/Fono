//! Shared build/runtime verifier for the closed, pinned inference package.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, io::Read, path::Path};

#[derive(Deserialize)]
pub struct Manifest {
    pub schema: String,
    pub protocol: u32,
    pub cnn: String,
    pub verifier: String,
    pub files: Vec<Entry>,
}
#[derive(Deserialize)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
pub fn hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|_| "missing asset")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| "unreadable asset")?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn regular(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "missing asset")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("linked asset".into());
        }
    }
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err("invalid asset type".into());
    }
    Ok(())
}
pub fn verify(root: &Path, expected: Option<&str>) -> Result<String, String> {
    regular(root, true)?;
    let manifest_path = root.join("manifest.json");
    regular(&manifest_path, false)?;
    if fs::metadata(&manifest_path)
        .map_err(|_| "missing manifest")?
        .len()
        > 1024 * 1024
    {
        return Err("oversized manifest".into());
    }
    let identity = hash(&manifest_path)?;
    if expected.is_some_and(|expected| identity != expected) {
        return Err("manifest identity changed".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|_| "missing manifest")?)
            .map_err(|_| "invalid manifest")?;
    if manifest.schema != "fono-lira-runtime@1"
        || manifest.protocol != 2
        || manifest.cnn != "a9c4e2595669605325b97b7aeec62781c58073e9966a3850d360328bddaa541c"
        || manifest.verifier != "d3ea4079b329755bcdd42618cec915aa4952554479eb4b0999689558305ee47e"
        || manifest.files.is_empty()
        || manifest.files.len() > 5000
    {
        return Err("runtime identity changed".into());
    }
    let mut files = BTreeSet::new();
    for entry in manifest.files {
        if entry.path.is_empty()
            || entry.path.contains(['\\', ':'])
            || entry
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || entry.path.starts_with('/')
            || entry.path == "manifest.json"
            || !files.insert(entry.path.to_lowercase())
        {
            return Err("unsafe or duplicate asset path".into());
        }
        let path = root.join(&entry.path);
        let mut parent = path.parent();
        while let Some(directory) = parent.filter(|parent| *parent != root) {
            regular(directory, true)?;
            parent = directory.parent();
        }
        regular(&path, false)?;
        if fs::metadata(&path).map_err(|_| "missing asset")?.len() != entry.size
            || hash(&path)? != entry.sha256
        {
            return Err("asset changed".into());
        }
    }
    let mut actual = BTreeSet::new();
    inventory(root, root, &mut actual)?;
    actual.remove("manifest.json");
    if actual != files
        || !files.contains("runtime/python.exe")
        || !files.contains("runtime/python312._pth")
        || !files.contains("worker/worker.py")
        || !files.contains("models/tone.onnx")
    {
        return Err("incomplete or unexpected package files".into());
    }
    Ok(identity)
}
fn inventory(root: &Path, directory: &Path, files: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|_| "unreadable package")? {
        let path = entry.map_err(|_| "unreadable package")?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| "missing package file")?;
        regular(&path, metadata.is_dir())?;
        if metadata.is_dir() {
            inventory(root, &path, files)?;
        } else {
            files.insert(
                path.strip_prefix(root)
                    .map_err(|_| "foreign path")?
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_lowercase(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Fixture {
        let fixture = Fixture(
            std::env::temp_dir().join(format!("fono-lira-package-{}", uuid::Uuid::new_v4())),
        );
        fs::create_dir(&fixture.0).unwrap();
        let mut entries = Vec::new();
        for name in [
            "runtime/python.exe",
            "runtime/python312._pth",
            "worker/worker.py",
            "models/tone.onnx",
        ] {
            let path = fixture.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"fixture only; never executable").unwrap();
            entries.push(serde_json::json!({"path":name,"size":fs::metadata(&path).unwrap().len(),"sha256":hash(&path).unwrap()}));
        }
        fs::write(fixture.0.join("manifest.json"), serde_json::to_vec(&serde_json::json!({
            "schema":"fono-lira-runtime@1", "protocol":2,
            "cnn":"a9c4e2595669605325b97b7aeec62781c58073e9966a3850d360328bddaa541c",
            "verifier":"d3ea4079b329755bcdd42618cec915aa4952554479eb4b0999689558305ee47e", "files":entries
        })).unwrap()).unwrap();
        fixture
    }
    #[test]
    fn signed_installer_manifest_pin_and_file_hashes_are_both_required() {
        let fixture = fixture();
        let identity = verify(&fixture.0, None).unwrap();
        assert!(verify(&fixture.0, Some(&identity)).is_ok());
        assert!(verify(&fixture.0, Some(&"0".repeat(64))).is_err());
        fs::write(fixture.0.join("worker/worker.py"), b"changed").unwrap();
        assert!(verify(&fixture.0, Some(&identity)).is_err());
    }
    #[test]
    fn missing_and_unlisted_python_modules_block_startup() {
        let fixture = fixture();
        fs::write(fixture.0.join("worker/sitecustomize.py"), b"unlisted").unwrap();
        assert!(verify(&fixture.0, None).is_err());
        fs::remove_file(fixture.0.join("worker/sitecustomize.py")).unwrap();
        fs::remove_file(fixture.0.join("runtime/python.exe")).unwrap();
        assert!(verify(&fixture.0, None).is_err());
    }
    #[test]
    fn traversal_and_case_collisions_are_rejected_before_opening_assets() {
        for name in [
            "../outside",
            "C:/outside",
            "worker/../outside",
            "worker\\outside",
            "RUNTIME/PYTHON.EXE",
        ] {
            let fixture = fixture();
            let path = fixture.0.join("manifest.json");
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let mut entry = manifest["files"][0].clone();
            entry["path"] = name.into();
            manifest["files"].as_array_mut().unwrap().push(entry);
            fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            assert!(verify(&fixture.0, None).is_err(), "{name}");
        }
    }
}
