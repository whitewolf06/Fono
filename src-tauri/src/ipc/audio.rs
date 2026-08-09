use crate::audio::AudioCapture;
use crate::error::AppResult;
use crate::types::DeviceInfo;

#[tauri::command]
pub fn list_audio_devices() -> AppResult<Vec<DeviceInfo>> {
    AudioCapture::list_input_devices()
}
