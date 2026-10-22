//! Explicit, local-only recordings for detector development. This does not train or enable wake.
mod storage;
pub(crate) mod testing;
pub(crate) use storage::{delete, list, read_audio, save, Sample, SampleRequest};
