//! Owning host ports. Guards survive cancellation and worker retirement.

use crate::{
    error::{SttError, SttResult},
    SttReadinessObserver, WorkerPaths,
};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub struct ActivityLease {
    _token: Arc<dyn Send + Sync>,
}
impl ActivityLease {
    pub fn new<T: Send + Sync + 'static>(token: T) -> Self {
        Self {
            _token: Arc::new(token),
        }
    }
}

pub trait ActivityGuard: Send + Sync {
    fn lease(&self) -> SttResult<ActivityLease>;
    /// Retain teardown already admitted by the host, even during shutdown.
    fn cleanup_lease(&self) -> ActivityLease;
}
#[derive(Default)]
pub struct NoopActivity;
impl ActivityGuard for NoopActivity {
    fn lease(&self) -> SttResult<ActivityLease> {
        Ok(ActivityLease::new(()))
    }
    fn cleanup_lease(&self) -> ActivityLease {
        ActivityLease::new(())
    }
}

/// Implementations own any locks needed to keep verified worker files stable.
pub trait GpuPackage: Send + Sync {
    fn worker_path(&self) -> &Path;
    fn identity(&self) -> &str;
    fn validate_layout(&self) -> SttResult<()>;
}

#[derive(Clone)]
pub struct GpuPackageHandle(Arc<dyn GpuPackage>);
impl GpuPackageHandle {
    pub fn new<T: GpuPackage + 'static>(package: T) -> Self {
        Self(Arc::new(package))
    }
    pub fn worker_path(&self) -> &Path {
        self.0.worker_path()
    }
    pub fn identity(&self) -> &str {
        self.0.identity()
    }
    pub fn validate_layout(&self) -> SttResult<()> {
        self.0.validate_layout()
    }
}
impl PartialEq for GpuPackageHandle {
    fn eq(&self, other: &Self) -> bool {
        self.worker_path() == other.worker_path() && self.identity() == other.identity()
    }
}
impl Eq for GpuPackageHandle {}
impl fmt::Debug for GpuPackageHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuPackageHandle")
            .field("identity", &self.identity())
            .finish_non_exhaustive()
    }
}
pub trait GpuPackageSource: Send + Sync {
    fn current(&self) -> Option<GpuPackageHandle>;
}
#[derive(Default)]
pub struct NoGpuPackages;
impl GpuPackageSource for NoGpuPackages {
    fn current(&self) -> Option<GpuPackageHandle> {
        None
    }
}

pub type ErrorMessage = Arc<dyn Fn(&SttError) -> String + Send + Sync>;

#[derive(Clone)]
pub struct SttEnvironment {
    pub models_dir: PathBuf,
    pub speech_vad_model: Option<PathBuf>,
    pub workers: WorkerPaths,
    pub gpu_packages: Arc<dyn GpuPackageSource>,
    pub activity: Arc<dyn ActivityGuard>,
    pub readiness: Option<SttReadinessObserver>,
    pub error_message: ErrorMessage,
}
impl Default for SttEnvironment {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::new(),
            speech_vad_model: None,
            workers: WorkerPaths::default(),
            gpu_packages: Arc::new(NoGpuPackages),
            activity: Arc::new(NoopActivity),
            readiness: None,
            error_message: Arc::new(SttError::user_message),
        }
    }
}

#[cfg(test)]
#[derive(Default, Clone)]
pub(crate) struct TestActivity {
    count: Arc<std::sync::atomic::AtomicUsize>,
}
#[cfg(test)]
impl TestActivity {
    pub(crate) fn is_busy(&self) -> bool {
        self.count.load(std::sync::atomic::Ordering::Acquire) != 0
    }
}
#[cfg(test)]
impl ActivityGuard for TestActivity {
    fn lease(&self) -> SttResult<ActivityLease> {
        Ok(self.cleanup_lease())
    }
    fn cleanup_lease(&self) -> ActivityLease {
        struct Token(Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Token {
            fn drop(&mut self) {
                self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        self.count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        ActivityLease::new(Token(self.count.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owning_lease_clones_hold_admission_until_the_last_background_owner_finishes() {
        let activity = TestActivity::default();
        let lease = activity.lease().unwrap();
        let background = lease.clone();
        drop(lease);
        assert!(activity.is_busy());
        drop(background);
        assert!(!activity.is_busy());
    }
}
