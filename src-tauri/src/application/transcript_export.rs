//! Desktop error adaptation for shared transcript export.
pub use fono_service::transcript_export::{cues, ExportFormat};

use crate::error::AppResult;
use fono_types::TranscriptionResult;

pub fn render(result: &TranscriptionResult, format: ExportFormat) -> AppResult<String> {
    fono_service::transcript_export::render(result, format).map_err(Into::into)
}
