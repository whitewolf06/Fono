//! Desktop adaptation for the shared segmented recognition algorithm.

pub use fono_stt::segmented_transcription::{append_text, transcribe_segments};

impl fono_stt::segmented_transcription::SegmentError for crate::error::AppError {
    fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }
}
