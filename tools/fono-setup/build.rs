use std::{env, fs, path::PathBuf};

fn main() {
    let crate_root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let repo_root = crate_root.join("../..");
    let package_path = repo_root.join("package.json");
    let channel_path = repo_root.join("src-tauri/update-channel.json");
    let tauri_config_path = repo_root.join("src-tauri/tauri.conf.json");
    let icon_path = repo_root.join("src-tauri/icons/icon.ico");
    let manifest_path = crate_root.join("windows.manifest");
    for path in [
        &package_path,
        &channel_path,
        &tauri_config_path,
        &icon_path,
        &manifest_path,
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(package_path).unwrap()).unwrap();
    let channel: serde_json::Value =
        serde_json::from_slice(&fs::read(channel_path).unwrap()).unwrap();
    let tauri_config: serde_json::Value =
        serde_json::from_slice(&fs::read(tauri_config_path).unwrap()).unwrap();
    assert_eq!(
        tauri_config["identifier"].as_str(),
        Some("com.fono.desktop"),
        "Update the Fono MSI upgrade-code guard when the app identity changes"
    );
    let version = package["version"].as_str().expect("Fono version");
    let endpoint = channel["endpoint"]
        .as_str()
        .expect("Public updater endpoint");
    let public_key = channel["publicKey"]
        .as_str()
        .expect("Public updater signing key");
    assert_eq!(
        endpoint,
        "https://github.com/whitewolf06/fono/releases/latest/download/latest.json"
    );
    assert!(!public_key.is_empty() && !public_key.contains(['\r', '\n']));
    println!("cargo:rustc-env=FONO_SETUP_VERSION={version}");
    println!("cargo:rustc-env=FONO_SETUP_ENDPOINT={endpoint}");
    println!("cargo:rustc-env=FONO_SETUP_PUBLIC_KEY={public_key}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let parts: Vec<u16> = version
            .split('.')
            .map(|part| part.parse().unwrap())
            .collect();
        assert_eq!(parts.len(), 3);
        let numeric_version =
            (u64::from(parts[0]) << 48) | (u64::from(parts[1]) << 32) | (u64::from(parts[2]) << 16);
        tauri_winres::WindowsResource::new()
            .set_icon_with_id(icon_path.to_str().unwrap(), "101")
            .set_manifest_file(manifest_path.to_str().unwrap())
            .set("ProductName", "Fono")
            .set("FileDescription", "Fono — online installer")
            .set("OriginalFilename", "FonoSetup.exe")
            .set("FileVersion", version)
            .set("ProductVersion", version)
            .set("LegalCopyright", "© 2026 Fono")
            .set_version_info(tauri_winres::VersionInfo::FILEVERSION, numeric_version)
            .set_version_info(tauri_winres::VersionInfo::PRODUCTVERSION, numeric_version)
            .compile()
            .expect("Compile Fono installer icon and manifest");
    }
}
