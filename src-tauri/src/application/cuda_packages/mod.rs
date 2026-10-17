//! Explicit installation of an exact-version signed optional CUDA package.
//!
//! The renderer never supplies a URL, public key, path or trusted manifest.
//! `begin` is called before queuing blocking I/O; its operation owns the update
//! lease until the blocking job and its cleanup have actually finished.
mod download;
mod manifest;
mod sealing;
mod storage;

use crate::error::{AppError, AppResult};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    fmt,
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

type Result<T> = std::result::Result<T, PackageError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageError {
    Configuration,
    Manifest,
    Signature,
    Compatibility,
    Hash,
    Rollback,
    Storage,
    Network,
    Unpublished,
    Cancelled,
}

impl PackageError {
    fn app(self) -> AppError {
        if self == Self::Cancelled {
            AppError::Cancelled("Установка пакета CUDA отменена.".into())
        } else {
            // Do not expose paths, signed asset URLs/tokens or source errors.
            AppError::Config(
                match self {
                    Self::Configuration => "Установка пакета CUDA недоступна в этой сборке.",
                    Self::Manifest => "Пакет CUDA имеет неверный формат.",
                    Self::Signature => "Не удалось подтвердить подпись пакета CUDA.",
                    Self::Compatibility => "Пакет CUDA несовместим с этой версией Fono.",
                    Self::Hash => "Проверка целостности пакета CUDA не пройдена.",
                    Self::Rollback => "Более старая или изменённая ревизия пакета CUDA отклонена.",
                    Self::Storage => "Не удалось подготовить локальные файлы пакета CUDA.",
                    Self::Network => {
                        "Не удалось загрузить пакет CUDA. Проверьте соединение и повторите попытку."
                    }
                    Self::Unpublished => "Для этой версии пакет CUDA пока не опубликован.",
                    Self::Cancelled => unreachable!(),
                }
                .into(),
            )
        }
    }
}

fn invalid() -> PackageError {
    PackageError::Manifest
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(PackageError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(any(windows, test))]
fn is_hardware_nvidia(vendor: u32, software: bool) -> bool {
    vendor == 0x10de && !software
}

/// Read only adapter descriptors on the owning blocking thread. No model,
/// worker, audio device or telemetry counter is opened for this UI hint.
#[cfg(windows)]
fn detect_nvidia() -> Option<bool> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND,
    };
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    for index in 0..32 {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return Some(false),
            Err(_) => return None,
        };
        let description = unsafe { adapter.GetDesc1() }.ok()?;
        if is_hardware_nvidia(
            description.VendorId,
            description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
        ) {
            return Some(true);
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_nvidia() -> Option<bool> {
    None
}

#[derive(Clone)]
pub struct VerifiedPackage {
    worker: PathBuf,
    manifest: manifest::Manifest,
    hash: String,
    _guards: Arc<Vec<File>>,
}

impl VerifiedPackage {
    pub fn worker_path(&self) -> &Path {
        &self.worker
    }
    pub fn revision(&self) -> u64 {
        self.manifest.revision
    }
    pub fn app_version(&self) -> &str {
        &self.manifest.app_version
    }
    pub fn manifest_hash(&self) -> &str {
        &self.hash
    }

    /// Cheap final layout and persistent-ACL check at worker spawn. Retained
    /// handles protect the signature/file bytes; the sealed payload ACL also
    /// prevents creation of unsigned app-local dependencies.
    pub fn validate_layout(&self) -> AppResult<()> {
        storage::validate_layout(self).map_err(PackageError::app)
    }
}
impl fmt::Debug for VerifiedPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedPackage")
            .field("app_version", &self.app_version())
            .field("revision", &self.revision())
            .field("manifest_hash", &self.hash)
            .finish()
    }
}
impl PartialEq for VerifiedPackage {
    fn eq(&self, other: &Self) -> bool {
        self.worker == other.worker && self.hash == other.hash
    }
}
impl Eq for VerifiedPackage {}

pub fn package_root() -> AppResult<PathBuf> {
    Ok(crate::state::app_data_dir()?.join("cuda-packages"))
}

/// Fresh signature, file-list/hash and antirollback verification. Keep the
/// returned guard alive until the actual worker process exits.
pub fn active_package(root: &Path) -> AppResult<Option<VerifiedPackage>> {
    if cfg!(feature = "cpu-only") {
        return Ok(None);
    }
    let trust = manifest::Trust::production().map_err(PackageError::app)?;
    storage::active(root, &trust).map_err(PackageError::app)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CudaPackagePhase {
    Unavailable,
    NotInstalled,
    Checking,
    Downloading,
    Verifying,
    Activating,
    Cancelling,
    Installed,
    Error,
    Cancelled,
}

impl CudaPackagePhase {
    fn message(self) -> &'static str {
        match self {
            Self::Unavailable => "Установка CUDA недоступна в этой сборке.",
            Self::NotInstalled => {
                "Пакет CUDA не установлен. Auto использует доступный Vulkan или CPU."
            }
            Self::Checking => "Проверяем подписанный пакет для этой версии Fono.",
            Self::Downloading => "Загружаем пакет CUDA.",
            Self::Verifying => "Проверяем файлы пакета CUDA.",
            Self::Activating => "Подключаем проверенный пакет CUDA.",
            Self::Cancelling => "Отменяем установку и завершаем текущую работу.",
            Self::Installed => "Пакет CUDA установлен и проверен.",
            Self::Error => "Не удалось установить пакет CUDA. Повторите попытку.",
            Self::Cancelled => "Установка пакета CUDA отменена.",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CudaPackageSnapshot {
    pub phase: CudaPackagePhase,
    pub operation_id: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub installed_revision: Option<u64>,
    pub nvidia_detected: Option<bool>,
    pub message: Option<String>,
}

struct Data {
    snapshot: CudaPackageSnapshot,
    running: Option<(String, Arc<AtomicBool>)>,
    active_package: Option<VerifiedPackage>,
    epoch: u64,
}
struct Shared {
    data: Mutex<Data>,
}

pub struct CudaPackageService {
    root: PathBuf,
    shared: Arc<Shared>,
}

/// An uncloneable admission ticket. Dropping an IPC future does not cancel or
/// release a ticket already moved into a blocking worker. Drop also handles a
/// failed queue dispatch, retaining single-flight until all cleanup is over.
pub struct InstallOperation {
    id: String,
    cancel: Arc<AtomicBool>,
    shared: Arc<Shared>,
    _lease: super::updates::activity::ActivityLease<'static>,
}

impl Drop for InstallOperation {
    fn drop(&mut self) {
        let mut data = self.shared.data.lock();
        if data.running.as_ref().is_some_and(|(id, _)| id == &self.id) {
            data.running = None;
            if matches!(
                data.snapshot.phase,
                CudaPackagePhase::Checking
                    | CudaPackagePhase::Downloading
                    | CudaPackagePhase::Verifying
                    | CudaPackagePhase::Activating
                    | CudaPackagePhase::Cancelling
            ) {
                data.snapshot.phase = CudaPackagePhase::Cancelled;
                data.snapshot.message = Some(CudaPackagePhase::Cancelled.message().into());
            }
        }
    }
}

impl CudaPackageService {
    pub fn new(root: PathBuf) -> Self {
        let available = cfg!(windows) && !cfg!(feature = "cpu-only");
        Self {
            root,
            shared: Arc::new(Shared {
                data: Mutex::new(Data {
                    snapshot: CudaPackageSnapshot {
                        phase: if available {
                            CudaPackagePhase::NotInstalled
                        } else {
                            CudaPackagePhase::Unavailable
                        },
                        operation_id: None,
                        downloaded_bytes: 0,
                        total_bytes: None,
                        installed_revision: None,
                        nvidia_detected: None,
                        message: Some(
                            if available {
                                CudaPackagePhase::NotInstalled
                            } else {
                                CudaPackagePhase::Unavailable
                            }
                            .message()
                            .into(),
                        ),
                    },
                    running: None,
                    active_package: None,
                    epoch: 0,
                }),
            }),
        }
    }

    pub fn snapshot(&self) -> CudaPackageSnapshot {
        self.shared.data.lock().snapshot.clone()
    }

    /// Cheap, in-memory discovery for UI and recovery routing. Refresh is a
    /// blocking startup action; this guard keeps verified bytes protected.
    pub fn verified_active(&self) -> Option<VerifiedPackage> {
        if cfg!(feature = "cpu-only") {
            return None;
        }
        self.shared.data.lock().active_package.clone()
    }

    /// Blocking startup refresh; `snapshot` itself performs no I/O.
    pub fn refresh_installed(&self) -> AppResult<()> {
        if cfg!(feature = "cpu-only") || !cfg!(windows) {
            return Ok(());
        }
        let ticket = {
            let data = self.shared.data.lock();
            if data.running.is_some() {
                return Ok(());
            }
            data.epoch
        };
        let nvidia_detected = detect_nvidia();
        self.publish_refresh(ticket, nvidia_detected, active_package(&self.root))
    }

    fn publish_refresh(
        &self,
        ticket: u64,
        nvidia_detected: Option<bool>,
        installed: AppResult<Option<VerifiedPackage>>,
    ) -> AppResult<()> {
        let mut data = self.shared.data.lock();
        // A slow startup verification must not replace a newer installation,
        // nor clear its cache when the old read eventually returns an error.
        if data.epoch != ticket || data.running.is_some() {
            return Ok(());
        }
        data.snapshot.nvidia_detected = nvidia_detected;
        let installed = match installed {
            Ok(installed) => installed,
            Err(error) => {
                data.active_package = None;
                data.snapshot.installed_revision = None;
                data.snapshot.phase = CudaPackagePhase::Error;
                data.snapshot.message = Some("Не удалось проверить установленный пакет CUDA. Auto использует доступный Vulkan или CPU.".into());
                return Err(error);
            }
        };
        data.snapshot.phase = if installed.is_some() {
            CudaPackagePhase::Installed
        } else {
            CudaPackagePhase::NotInstalled
        };
        data.snapshot.message = Some(data.snapshot.phase.message().into());
        data.snapshot.installed_revision = installed.as_ref().map(|package| package.revision());
        data.active_package = installed;
        Ok(())
    }

    pub fn begin(&self, operation_id: String) -> AppResult<InstallOperation> {
        if cfg!(feature = "cpu-only") || !cfg!(windows) {
            return Err(PackageError::Configuration.app());
        }
        if operation_id.is_empty()
            || operation_id.len() > 64
            || !operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(PackageError::Configuration.app());
        }
        // Always acquire the global admission fence before service state.
        let lease = super::updates::activity::lease()?;
        let mut data = self.shared.data.lock();
        if data.running.is_some() {
            return Err(AppError::Busy(
                "Установка пакета CUDA уже выполняется.".into(),
            ));
        }
        data.epoch = data
            .epoch
            .checked_add(1)
            .ok_or_else(|| PackageError::Configuration.app())?;
        let cancel = Arc::new(AtomicBool::new(false));
        data.running = Some((operation_id.clone(), Arc::clone(&cancel)));
        data.snapshot.phase = CudaPackagePhase::Checking;
        data.snapshot.operation_id = Some(operation_id.clone());
        data.snapshot.downloaded_bytes = 0;
        data.snapshot.total_bytes = None;
        data.snapshot.message = Some(CudaPackagePhase::Checking.message().into());
        Ok(InstallOperation {
            id: operation_id,
            cancel,
            shared: Arc::clone(&self.shared),
            _lease: lease,
        })
    }

    pub fn cancel(&self, operation_id: &str) -> CudaPackageSnapshot {
        let mut data = self.shared.data.lock();
        if let Some((id, cancel)) = data.running.as_ref() {
            if id == operation_id && data.snapshot.phase != CudaPackagePhase::Installed {
                cancel.store(true, Ordering::Release);
                data.snapshot.phase = CudaPackagePhase::Cancelling;
                data.snapshot.message = Some(CudaPackagePhase::Cancelling.message().into());
            }
        }
        data.snapshot.clone()
    }

    /// Invoke in spawn_blocking. `probe` uses production hello/caps and waits
    /// for supervised process cleanup. `activate` owns the configuration-idle
    /// transaction and calls `commit` as its LAST fallible action. No pointer
    /// or high-water state changes before that callback explicitly commits.
    pub fn install_blocking(
        &self,
        operation: InstallOperation,
        probe: impl FnOnce(&VerifiedPackage) -> AppResult<()>,
        activate: impl FnOnce(&VerifiedPackage, &mut dyn FnMut() -> AppResult<()>) -> AppResult<()>,
    ) -> AppResult<CudaPackageSnapshot> {
        if !Arc::ptr_eq(&operation.shared, &self.shared) {
            return Err(PackageError::Configuration.app());
        }
        let result = self.install(&operation, probe, activate);
        let mut data = self.shared.data.lock();
        // commit is the linearization point. A cancel after it cannot undo a
        // successfully published package or apply to the next generation.
        if let Err(error) = &result {
            if data.snapshot.phase != CudaPackagePhase::Installed {
                data.snapshot.phase = if operation.cancel.load(Ordering::Acquire) {
                    CudaPackagePhase::Cancelled
                } else {
                    CudaPackagePhase::Error
                };
                // Arbitrary activation/probe errors may contain paths or
                // worker stderr. Never publish them as status text.
                data.snapshot.message = Some(match error {
                    AppError::Config(message)
                        if message == "Для этой версии пакет CUDA пока не опубликован." =>
                    {
                        message.clone()
                    }
                    _ => data.snapshot.phase.message().into(),
                });
            }
        }
        let snapshot = data.snapshot.clone();
        drop(data);
        // Operation Drop clears single-flight only after Staging, file lock
        // and callback-owned probe/native cleanup have all finished.
        drop(operation);
        result.map(|_| snapshot)
    }

    fn phase(&self, operation: &InstallOperation, phase: CudaPackagePhase) -> Result<()> {
        check_cancel(&operation.cancel)?;
        let mut data = self.shared.data.lock();
        if !data
            .running
            .as_ref()
            .is_some_and(|(id, _)| id == &operation.id)
        {
            return Err(PackageError::Cancelled);
        }
        if data.snapshot.phase != CudaPackagePhase::Cancelling {
            data.snapshot.phase = phase;
            data.snapshot.message = Some(phase.message().into());
        }
        Ok(())
    }

    fn install(
        &self,
        operation: &InstallOperation,
        probe: impl FnOnce(&VerifiedPackage) -> AppResult<()>,
        activate: impl FnOnce(&VerifiedPackage, &mut dyn FnMut() -> AppResult<()>) -> AppResult<()>,
    ) -> AppResult<()> {
        check_cancel(&operation.cancel).map_err(PackageError::app)?;
        let trust = manifest::Trust::production().map_err(PackageError::app)?;
        let _lock = storage::install_lock(&self.root).map_err(PackageError::app)?;
        let client = download::client().map_err(PackageError::app)?;
        let name = "fono-cuda-windows-x86_64.json";
        let origin = download::asset(&trust.app_version, name).map_err(PackageError::app)?;
        let bytes = download::small(&client, &origin, manifest::MAX_MANIFEST, &operation.cancel)
            .map_err(PackageError::app)?;
        let origin = download::asset(&trust.app_version, &format!("{name}.sig"))
            .map_err(PackageError::app)?;
        let signature =
            download::small(&client, &origin, manifest::MAX_SIGNATURE, &operation.cancel)
                .map_err(PackageError::app)?;
        let signature = String::from_utf8(signature).map_err(|_| PackageError::Signature.app())?;
        let manifest = manifest::verify(&bytes, &signature, &trust).map_err(PackageError::app)?;
        let hash = manifest::hash(&bytes);
        storage::allow(
            &storage::state(&self.root).map_err(PackageError::app)?,
            &manifest,
            &hash,
        )
        .map_err(PackageError::app)?;
        self.phase(operation, CudaPackagePhase::Downloading)
            .map_err(PackageError::app)?;
        let mut stage = storage::Staging::new(&self.root).map_err(PackageError::app)?;
        let archive = stage.path.join("archive.tar.bz2");
        download::archive(
            &client,
            &manifest,
            &archive,
            &operation.cancel,
            |downloaded, total| {
                let mut data = self.shared.data.lock();
                data.snapshot.downloaded_bytes = downloaded;
                data.snapshot.total_bytes = Some(total);
            },
        )
        .map_err(PackageError::app)?;
        self.phase(operation, CudaPackagePhase::Verifying)
            .map_err(PackageError::app)?;
        storage::extract(
            &archive,
            &mut stage,
            &manifest,
            &bytes,
            &signature,
            &operation.cancel,
        )
        .map_err(PackageError::app)?;
        std::fs::remove_file(&archive).map_err(|_| PackageError::Storage.app())?;
        let slot = storage::promote(&self.root, &mut stage, &hash).map_err(PackageError::app)?;
        let package = storage::verify_slot(&self.root, &slot, &trust, &operation.cancel)
            .map_err(PackageError::app)?;
        self.activate_prepared(operation, &package, probe, activate)
    }

    fn activate_prepared(
        &self,
        operation: &InstallOperation,
        package: &VerifiedPackage,
        probe: impl FnOnce(&VerifiedPackage) -> AppResult<()>,
        activate: impl FnOnce(&VerifiedPackage, &mut dyn FnMut() -> AppResult<()>) -> AppResult<()>,
    ) -> AppResult<()> {
        probe(package)?;
        self.phase(operation, CudaPackagePhase::Activating)
            .map_err(PackageError::app)?;
        let mut committed = false;
        let mut commit = || {
            let mut data = self.shared.data.lock();
            check_cancel(&operation.cancel).map_err(PackageError::app)?;
            if committed {
                return Err(PackageError::Configuration.app());
            }
            let next_epoch = data
                .epoch
                .checked_add(1)
                .ok_or_else(|| PackageError::Configuration.app())?;
            storage::commit(&self.root, package).map_err(PackageError::app)?;
            data.epoch = next_epoch;
            committed = true;
            data.snapshot.phase = CudaPackagePhase::Installed;
            data.snapshot.installed_revision = Some(package.revision());
            data.active_package = Some(package.clone());
            data.snapshot.message = Some(CudaPackagePhase::Installed.message().into());
            Ok(())
        };
        activate(package, &mut commit)?;
        if !committed {
            return Err(PackageError::Configuration.app());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
