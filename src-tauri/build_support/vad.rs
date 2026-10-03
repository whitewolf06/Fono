use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const MODEL: &str = "silero_vad.onnx";
const SHA256: &str = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6";

/// Match the bundled resource layout for directly launched Cargo binaries too.
pub fn stage(manifest: &Path, profile: &Path) {
    let source = manifest.join("resources/vad").join(MODEL);
    println!("cargo:rerun-if-changed={}", source.display());
    let bytes = fs::read(&source)
        .unwrap_or_else(|error| panic!("required speech VAD model {}: {error}", source.display()));
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        SHA256,
        "speech VAD checksum mismatch; update the model and manifest together"
    );
    let directory = profile.join("resources/vad");
    fs::create_dir_all(&directory).expect("create speech VAD resource directory");
    let destination = directory.join(MODEL);
    if fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
        fs::write(&destination, bytes).expect("stage speech VAD resource");
    }
}
