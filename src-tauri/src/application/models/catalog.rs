//! Desktop model-directory adapter for the shared catalog.

use crate::{error::AppResult, state};
pub use fono_stt::models::catalog::SttModelInfo;

pub fn list_stt_models() -> AppResult<Vec<SttModelInfo>> {
    Ok(fono_stt::models::catalog::list_stt_models(
        &state::models_dir()?,
    )?)
}
