use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let package_json = manifest_dir.join("../../package.json");
    println!("cargo:rerun-if-changed={}", package_json.display());
    let contents = std::fs::read_to_string(&package_json).expect("read product package.json");
    let package: serde_json::Value =
        serde_json::from_str(&contents).expect("parse product package.json");
    let version = package["version"]
        .as_str()
        .expect("product package.json must contain version");
    println!("cargo:rustc-env=FONO_PRODUCT_VERSION={version}");
}
