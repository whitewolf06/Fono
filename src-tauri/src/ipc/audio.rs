use crate::audio::AudioCapture;
use crate::error::AppResult;
use crate::types::DeviceInfo;

#[tauri::command]
pub async fn list_audio_devices() -> AppResult<Vec<DeviceInfo>> {
    // CPAL creates and drops its WASAPI/COM objects on the same blocking thread.
    super::blocking::run(AudioCapture::list_input_devices).await
}

#[tauri::command]
pub async fn preview_recording_sounds(app: tauri::AppHandle, volume: u8) -> AppResult<()> {
    super::blocking::run(move || crate::application::recording_sounds::preview(&app, volume)).await
}
