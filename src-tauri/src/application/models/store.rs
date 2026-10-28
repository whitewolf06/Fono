//! Desktop error adapter; verified publication lives in the shared engine.

use crate::error::{AppError, AppResult};
use std::path::Path;
#[cfg(test)]
use std::{io::Read, sync::atomic::AtomicBool};

#[cfg(test)]
pub(super) fn install_verified_model(
    reader: &mut impl Read,
    target: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    cancellation: &AtomicBool,
    progress: impl FnMut(u64),
    verifying: impl FnOnce(),
) -> AppResult<fono_stt::models::download::DownloadReceipt> {
    fono_stt::models::store::install_verified_model(
        reader,
        target,
        expected_bytes,
        expected_sha256,
        cancellation,
        progress,
        verifying,
    )
    .map_err(AppError::from)
}

pub(super) fn remove_model_file(target: &Path, selected: Option<&str>) -> AppResult<()> {
    fono_stt::models::store::remove_model_file(target, selected).map_err(AppError::from)
}

#[cfg(test)]
#[path = "store_integration_tests.rs"]
mod tests;
