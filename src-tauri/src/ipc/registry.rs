//! Single registry for the desktop IPC contract.

pub(crate) fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // state
        crate::ipc::desktop_v3::get_runtime_ready,
        crate::ipc::desktop_v3::get_ui_locale,
        crate::ipc::desktop_v3::get_desktop_snapshot,
        crate::ipc::memory::get_stt_memory_status,
        crate::ipc::desktop_v3::improve_text,
        crate::ipc::desktop_v3::clear_speech_analytics,
        crate::ipc::desktop_v3::clear_application_context,
        crate::ipc::desktop_v3::recommend_speech,
        crate::ipc::desktop_v3::get_microphone_sample,
        crate::ipc::desktop_v3::test_microphone_device,
        crate::ipc::desktop_v3::remove_whisper_model,
        crate::ipc::desktop_v3::get_selected_model_metadata,
        crate::ipc::system::get_build_info,
        #[cfg(feature = "experimental-live")]
        crate::ipc::dictation::get_live_dictation,
        #[cfg(feature = "experimental-live")]
        crate::ipc::dictation::resume_live_insertion,
        crate::ipc::dictation::start_dictation,
        crate::ipc::dictation::stop_dictation,
        crate::ipc::dictation::finish_overlay_dictation,
        crate::ipc::dictation::acknowledge_overlay_processing_flush,
        crate::ipc::dictation::confirm_dictation,
        crate::ipc::dictation::cancel_dictation,
        crate::ipc::dictation::dismiss_dictation_overlay,
        crate::ipc::dictation::transcribe_test,
        crate::ipc::dictation::get_pending_dictation,
        crate::ipc::dictation::resolve_pending_dictation,
        crate::ipc::dictation::update_overlay_processing_choice,
        crate::ipc::system::get_dictation_history,
        crate::ipc::history_rewrite::get_history_rewrite_context,
        crate::ipc::history_rewrite::create_history_rewrite,
        crate::ipc::history_rewrite::cancel_history_rewrite,
        crate::ipc::history_rewrite::delete_history_rewrite,
        crate::ipc::system::clear_dictation_history,
        crate::ipc::system::delete_dictation_history_entry,
        crate::ipc::system::set_dictation_history_entry_analytics_included,
        crate::ipc::system::get_speech_session_analysis,
        crate::ipc::system::get_speech_period_report,
        crate::ipc::trainer::get_trainer_report,
        crate::ipc::trainer::analyze_trainer_period,
        crate::ipc::trainer::cancel_trainer_period,
        crate::ipc::service::get_local_transcription_service_snapshot,
        crate::ipc::service::set_local_transcription_service_enabled,
        crate::ipc::service::cancel_local_transcription_job,
        crate::ipc::service::clear_local_transcription_history,
        crate::ipc::service::copy_local_transcription_api_token,
        crate::ipc::files::choose_transcription_files,
        crate::ipc::files::submit_transcription_files,
        crate::ipc::files::export_transcription_file,
        crate::ipc::snippets::import_snippets,
        crate::ipc::snippets::export_snippets,
        crate::ipc::text::copy_dictation_text,
        crate::ipc::text::reinsert_dictation,
        crate::ipc::voice::get_pending_command_proposal,
        crate::ipc::voice::cancel_voice_command,
        crate::ipc::voice::confirm_voice_command,
        // audio
        crate::ipc::audio::list_audio_devices,
        crate::ipc::audio::preview_recording_sounds,
        // whisper
        crate::ipc::models::list_whisper_models,
        crate::ipc::models::list_stt_models,
        crate::ipc::models::download_stt_model,
        crate::ipc::models::set_stt_model,
        crate::ipc::models::remove_stt_model,
        crate::ipc::models::download_whisper_model,
        crate::ipc::models::set_whisper_model,
        // kws wake word model
        #[cfg(feature = "wake-word")]
        crate::ipc::models::is_kws_model_downloaded,
        #[cfg(feature = "wake-word")]
        crate::ipc::models::download_kws_model,
        crate::ipc::models::cancel_model_download,
        // llm
        crate::ipc::llm::test_llm_profile,
        crate::ipc::llm::list_llm_profile_models,
        crate::ipc::processing::get_processing_prompt_catalog,
        crate::ipc::processing::preview_processing_text,
        crate::ipc::processing_capture::start_processing_test_capture,
        crate::ipc::processing_capture::finish_processing_test_capture,
        crate::ipc::processing_capture::cancel_processing_test_capture,
        crate::ipc::wake_samples::list_wake_samples,
        crate::ipc::wake_samples::start_wake_sample,
        crate::ipc::wake_samples::finish_wake_sample,
        crate::ipc::wake_samples::cancel_wake_sample,
        crate::ipc::wake_samples::read_wake_sample,
        crate::ipc::wake_samples::delete_wake_sample,
        crate::ipc::wake_samples::wake_sample_test_available,
        crate::ipc::wake_samples::test_wake_sample,
        // settings
        crate::ipc::settings::get_settings,
        crate::ipc::settings::get_acceleration_capabilities,
        crate::ipc::settings::get_stt_readiness,
        crate::ipc::settings::get_stt_health,
        crate::ipc::settings::save_settings,
        crate::ipc::trigger_capture::begin_trigger_capture,
        crate::ipc::trigger_capture::end_trigger_capture,
        crate::ipc::selection::get_selection_capture,
        crate::ipc::selection::cancel_selection_capture,
        crate::ipc::selection::start_selection_instruction,
        crate::ipc::selection::stop_selection_instruction,
        crate::ipc::selection::cancel_selection_edit,
        crate::ipc::selection::transform_selection,
        crate::ipc::selection::arm_selection_replace,
        crate::ipc::selection::cancel_selection_delivery,
        crate::ipc::selection::copy_selection_result,
        crate::ipc::selection::insert_selection_result,
        // overlay
        crate::ipc::settings::save_overlay_position,
        crate::overlay::get_overlay_preview,
        crate::overlay::set_overlay_layout,
        crate::overlay::show_overlay_preview,
        crate::overlay::reset_overlay_position,
        crate::overlay::position_overlay,
        crate::overlay::hide_overlay_preview,
        // диагностика
        crate::ipc::diagnostics::get_recent_logs,
        crate::ipc::diagnostic_report::get_diagnostic_report,
        crate::ipc::updates::get_update_status,
        crate::ipc::cuda_packages::get_cuda_package_status,
        crate::ipc::cuda_packages::install_cuda_package,
        crate::ipc::cuda_packages::cancel_cuda_package,
        crate::ipc::updates::check_for_updates,
        crate::ipc::updates::install_update,
        crate::ipc::updates::cancel_update_download,
        crate::ipc::updates::set_update_checks_enabled,
        crate::ipc::project::open_project_site,
        crate::ipc::project::open_feedback_form,
        crate::ipc::diagnostics::clear_logs,
        crate::ipc::diagnostics::test_microphone,
        // wake word
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::get_wake_word_status,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::get_wake_word_capabilities,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::get_wake_word_diagnostics,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::test_wake_word_model,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::record_wake_word_sample,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::recognize_wake_word_sample,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::get_wake_calibration_status,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::start_wake_calibration,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::record_wake_calibration_sample,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::cancel_wake_calibration,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::get_wake_profile_validation_status,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::start_wake_profile_validation,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::record_wake_profile_validation_sample,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::enable_wake_word,
        #[cfg(feature = "wake-word")]
        crate::ipc::wake::disable_wake_word,
    ]
}
