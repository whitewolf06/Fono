use crate::application::gpu_memory::{self, SttMemoryStatus};
use tauri::AppHandle;

#[tauri::command]
pub(crate) fn get_stt_memory_status(app: AppHandle) -> SttMemoryStatus {
    gpu_memory::status(&app)
}
