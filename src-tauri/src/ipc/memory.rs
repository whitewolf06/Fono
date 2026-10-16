use crate::application::gpu_memory::{self, SttMemoryStatus};
use crate::error::AppResult;
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn get_stt_memory_status(app: AppHandle) -> AppResult<SttMemoryStatus> {
    // The residency query may reap a retired native worker.
    super::blocking::run(move || Ok(gpu_memory::status(&app))).await
}
