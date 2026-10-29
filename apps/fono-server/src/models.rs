//! Server model IDs and disk inventory over the shared, pinned STT catalog.
//!
//! Disk verification is distinct from runtime readiness: only `ModelPool` can
//! report whether a loaded recognizer is ready to accept speech.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fono_stt::models::download::DownloadProgress;
use fono_stt::models::server_store;
use fono_stt::offline_models::{self, OfflineModelSpec};
use fono_stt::ModelResidency;
use fono_types::{ModelId, SttModelSelection, WhisperArtifact};

use crate::config::{self, ServerConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    Missing,
    PresentUnverified,
    Verified,
    Invalid,
}

impl DiskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::PresentUnverified => "present_unverified",
            Self::Verified => "verified",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelListing {
    pub id: String,
    pub state: DiskState,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ModelVerification {
    pub id: String,
    pub state: DiskState,
    pub detail: Option<String>,
}

/// A pure startup plan. `serve` will use these selections with `ModelPool` in
/// SRV-05; constructing the plan does not load weights or start HTTP routes.
#[derive(Debug, Clone)]
pub struct PreloadPlan {
    pub id: String,
    pub selection: SttModelSelection,
    pub device: String,
    pub residency: ModelResidency,
}

#[derive(Clone)]
enum ManagedModel {
    Whisper(WhisperArtifact),
    Offline(OfflineModelSpec),
}

impl ManagedModel {
    fn id(&self) -> String {
        match self {
            Self::Whisper(artifact) => format!("whisper:{}", artifact.variant),
            Self::Offline(spec) => spec.id.public_id(),
        }
    }

    fn model_id(&self) -> ModelId {
        match self {
            Self::Whisper(artifact) => artifact.id(),
            Self::Offline(spec) => spec.id.clone(),
        }
    }

    fn component(&self) -> &str {
        match self {
            Self::Whisper(artifact) => &artifact.filename,
            Self::Offline(spec) => &spec.directory,
        }
    }

    fn path(&self, directory: &Path) -> PathBuf {
        directory.join(self.component())
    }

    fn bytes(&self) -> u64 {
        match self {
            Self::Whisper(artifact) => artifact.bytes,
            Self::Offline(spec) => spec.files.iter().map(|artifact| artifact.bytes).sum(),
        }
    }

    fn present_state(&self, directory: &Path) -> Result<DiskState, String> {
        let path = self.path(directory);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(DiskState::Missing),
            Err(error) => return Err(format!("Cannot inspect model path: {error}")),
        };
        if metadata.file_type().is_symlink() {
            return Ok(DiskState::Invalid);
        }
        match self {
            Self::Whisper(artifact) => {
                Ok(if metadata.is_file() && metadata.len() == artifact.bytes {
                    DiskState::PresentUnverified
                } else {
                    DiskState::Invalid
                })
            }
            Self::Offline(spec) => Ok(
                if metadata.is_dir() && fono_stt::models::offline_package::is_installed(&path, spec)
                {
                    DiskState::PresentUnverified
                } else {
                    DiskState::Invalid
                },
            ),
        }
    }

    fn verify(&self, directory: &Path) -> Result<bool, fono_stt::error::SttError> {
        match self {
            Self::Whisper(artifact) => server_store::verify_whisper(directory, artifact),
            Self::Offline(spec) => server_store::verify_offline(directory, spec),
        }
    }
}

fn safe_component(component: &str) -> bool {
    !component.is_empty()
        && component != "."
        && component != ".."
        && !component.contains(['/', '\\', ':', '\0'])
}

fn listing_directory_exists(directory: &Path) -> Result<bool, String> {
    // Match the store's path policy before reading any model entry. Walking
    // from the root also rejects a symlink in an existing parent directory.
    for ancestor in directory.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("Cannot inspect models.dir: {error}")),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("models.dir must be a real directory without symlink parents".into());
        }
    }
    Ok(true)
}

fn catalog() -> Result<Vec<ManagedModel>, String> {
    let whisper = WhisperArtifact::all().map_err(|_| "Whisper catalog is unavailable")?;
    let offline = offline_models::specifications().map_err(|_| "Offline catalog is unavailable")?;
    let mut models = Vec::with_capacity(whisper.len() + offline.len());
    models.extend(whisper.into_iter().map(ManagedModel::Whisper));
    models.extend(offline.into_iter().map(ManagedModel::Offline));
    let mut ids = std::collections::HashSet::new();
    let mut components = std::collections::HashSet::new();
    for model in &models {
        if !safe_component(model.component())
            || !ids.insert(model.id())
            || !components.insert(model.component())
        {
            return Err("Built-in model catalog contains an invalid entry".into());
        }
    }
    models.sort_by_key(ManagedModel::id);
    Ok(models)
}

fn resolve(id: &str) -> Result<ManagedModel, String> {
    // Exact matching against the pinned catalog rejects aliases, unsupported
    // variants and any ID that could become a filesystem path component.
    catalog()?
        .into_iter()
        .find(|model| model.id() == id)
        .ok_or_else(|| "Unknown managed model ID".into())
}

/// Validate all configured references against the pinned catalog without
/// touching disk or loading a recognizer. Errors identify the config field but
/// never echo an untrusted identifier.
pub fn validate_configuration(config: &ServerConfig) -> Result<(), String> {
    let known: std::collections::HashSet<_> =
        catalog()?.into_iter().map(|model| model.id()).collect();
    if !known.contains(&config.models.default) {
        return Err("models.default: unknown managed model ID".into());
    }
    for (index, id) in config.models.preload.iter().enumerate() {
        if !known.contains(id) {
            return Err(format!("models.preload[{index}]: unknown managed model ID"));
        }
    }
    for (index, placement) in config.models.placement.iter().enumerate() {
        if !known.contains(&placement.model) {
            return Err(format!(
                "models.placement[{index}].model: unknown managed model ID"
            ));
        }
        if placement.device != "cpu"
            && resolve(&placement.model)?.model_id().engine != fono_types::SttEngine::Whisper
        {
            return Err(format!(
                "models.placement[{index}].device: offline models require cpu placement"
            ));
        }
    }
    Ok(())
}

/// Cheap inventory: `present_unverified` means size/layout matched, not SHA or
/// runtime readiness. `verify` performs full checksum validation explicitly.
pub fn list(config: &ServerConfig) -> Result<Vec<ModelListing>, String> {
    let directory_exists = listing_directory_exists(&config.models.dir)?;
    catalog()?
        .into_iter()
        .map(|model| {
            Ok(ModelListing {
                id: model.id(),
                state: if directory_exists {
                    model.present_state(&config.models.dir)?
                } else {
                    DiskState::Missing
                },
                bytes: model.bytes(),
            })
        })
        .collect()
}

/// Verify every present known artifact. Missing entries are returned so the
/// caller can reject absent `models.default` or `models.preload` entries.
pub fn verify(config: &ServerConfig) -> Result<Vec<ModelVerification>, String> {
    catalog()?
        .into_iter()
        .map(|model| {
            let (state, detail) = match model.verify(&config.models.dir) {
                Ok(true) => (DiskState::Verified, None),
                Ok(false) => (DiskState::Missing, None),
                Err(error) => (DiskState::Invalid, Some(error.to_string())),
            };
            Ok(ModelVerification {
                id: model.id(),
                state,
                detail,
            })
        })
        .collect()
}

pub fn pull(
    config: &ServerConfig,
    id: &str,
    progress: &dyn DownloadProgress,
) -> Result<(), String> {
    let model = resolve(id)?;
    let cancellation = AtomicBool::new(false);
    match model {
        ManagedModel::Whisper(artifact) => {
            server_store::pull_whisper(&config.models.dir, &artifact, &cancellation, progress)
        }
        ManagedModel::Offline(spec) => {
            server_store::pull_offline(&config.models.dir, &spec, &cancellation, progress)
        }
    }
    .map_err(|error| error.to_string())
}

pub fn remove(config: &ServerConfig, id: &str) -> Result<(), String> {
    let model = resolve(id)?;
    validate_configuration(config)?;
    if config.models.default == id {
        return Err("Cannot remove models.default".into());
    }
    for preload in &config.models.preload {
        if preload == id {
            return Err("Cannot remove a model in models.preload".into());
        }
    }
    match model {
        ManagedModel::Whisper(artifact) => {
            server_store::remove_whisper(&config.models.dir, &artifact)
        }
        ManagedModel::Offline(spec) => server_store::remove_offline(&config.models.dir, &spec),
    }
    .map_err(|error| error.to_string())
}

pub fn resolve_preload(config: &ServerConfig) -> Result<Vec<PreloadPlan>, String> {
    validate_configuration(config)?;
    config
        .models
        .preload
        .iter()
        .map(|id| {
            let model = resolve(id)?;
            let placement = config
                .models
                .placement
                .iter()
                .find(|placement| placement.model == *id);
            let device =
                placement.map_or_else(|| "cpu".to_owned(), |placement| placement.device.clone());
            let residency = match placement.map(|placement| placement.residency) {
                None | Some(config::ModelResidency::Resident) => ModelResidency::Resident,
                Some(config::ModelResidency::IdleUnload) => ModelResidency::IdleUnload(
                    placement
                        .and_then(|placement| placement.idle_unload_after)
                        .ok_or("models.placement.idle_unload_after is required")?,
                ),
            };
            let path = model.path(&config.models.dir);
            let local_path = path
                .to_str()
                .ok_or("models.dir must be a UTF-8 path")?
                .to_owned();
            Ok(PreloadPlan {
                id: id.clone(),
                selection: SttModelSelection {
                    id: model.model_id(),
                    local_path,
                },
                device,
                residency,
            })
        })
        .collect()
}

/// Resolve a single installed model after checking its pinned artifact.
/// This never downloads a model or accepts a caller-controlled filesystem path.
pub fn resolve_installed(config: &ServerConfig, id: &str) -> Result<PreloadPlan, String> {
    let model = resolve(id)?;
    match model.verify(&config.models.dir) {
        Ok(true) => {}
        Ok(false) => return Err("Requested model is not installed".into()),
        Err(_) => return Err("Requested model failed artifact verification".into()),
    }
    let mut selection = config.clone();
    selection.models.preload = vec![id.to_owned()];
    resolve_preload(&selection)?
        .into_iter()
        .next()
        .ok_or_else(|| "Requested model has no runtime plan".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn sample_config() -> ServerConfig {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/server.toml.example");
        config::load_and_validate_with_env(&path, std::iter::empty::<(OsString, OsString)>())
            .unwrap()
    }

    #[test]
    fn catalog_uses_server_ids_without_changing_desktop_whisper_ids() {
        let catalog = catalog().unwrap();
        assert!(catalog
            .iter()
            .any(|model| model.id() == "whisper:large_turbo"));
        assert!(catalog
            .iter()
            .any(|model| model.id() == "giga_am:v3_e2e_rnnt_int8"));
        assert!(catalog
            .iter()
            .any(|model| model.id().starts_with("parakeet:")));
        assert!(resolve("whisper:large_turbo").is_ok());
        assert!(resolve("large_turbo").is_err());
        assert!(resolve("whisper:../large_turbo").is_err());
        assert!(resolve("giga_am:unknown").is_err());
    }

    #[test]
    fn empty_model_directory_is_not_misreported_as_runtime_ready() {
        let mut config = sample_config();
        config.models.dir = std::env::temp_dir().join(format!(
            "fono-server-missing-models-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let entries = list(&config).unwrap();
        assert!(entries
            .iter()
            .all(|entry| entry.state == DiskState::Missing));
        let verified = verify(&config).unwrap();
        assert!(verified
            .iter()
            .all(|entry| entry.state == DiskState::Missing));
        assert!(verified.iter().any(|entry| {
            entry.id == config.models.default && entry.state == DiskState::Missing
        }));
    }

    #[cfg(unix)]
    #[test]
    fn listing_rejects_symlinked_model_directory_and_parent() {
        use std::os::unix::fs::symlink;

        let mut config = sample_config();
        let root = std::env::temp_dir().join(format!(
            "fono-server-listing-links-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let real = root.join("real");
        std::fs::create_dir_all(real.join("models")).unwrap();
        let parent_link = root.join("parent-link");
        symlink(&real, &parent_link).unwrap();
        config.models.dir = parent_link.join("models");
        assert!(list(&config).unwrap_err().contains("models.dir"));

        let directory_link = root.join("models-link");
        symlink(real.join("models"), &directory_link).unwrap();
        config.models.dir = directory_link;
        assert!(list(&config).unwrap_err().contains("models.dir"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sample_preload_plan_resolves_paths_device_and_residency() {
        let config = sample_config();
        let plan = resolve_preload(&config).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, "whisper:large_turbo");
        assert_eq!(plan[0].device, "cpu");
        assert_eq!(plan[0].residency, ModelResidency::Resident);
        assert!(plan[0]
            .selection
            .local_path
            .ends_with("ggml-large-v3-turbo.bin"));
        let mut invalid = config;
        invalid.models.preload = vec!["whisper:no-such-model".into()];
        assert!(resolve_preload(&invalid).is_err());
    }

    #[test]
    fn active_default_and_preload_cannot_be_removed() {
        let mut config = sample_config();
        assert!(remove(&config, "whisper:large_turbo")
            .unwrap_err()
            .contains("models.default"));
        config.models.default = "whisper:base".into();
        assert!(remove(&config, "whisper:large_turbo")
            .unwrap_err()
            .contains("models.preload"));
    }

    #[test]
    fn configured_model_references_have_field_specific_errors() {
        let mut config = sample_config();
        config.models.default = "private-unknown-id".into();
        let error = validate_configuration(&config).unwrap_err();
        assert!(error.contains("models.default"));
        assert!(!error.contains("private-unknown-id"));

        config = sample_config();
        config.models.preload = vec!["private-unknown-id".into()];
        assert!(validate_configuration(&config)
            .unwrap_err()
            .contains("models.preload[0]"));

        config = sample_config();
        config.models.placement[0].model = "private-unknown-id".into();
        assert!(validate_configuration(&config)
            .unwrap_err()
            .contains("models.placement[0].model"));
    }
}
