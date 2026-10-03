#pragma once
#include "whisper.h"
#include "json.hpp"
#include "pcm.hpp"
#include "timestamps.hpp"
#include <atomic>
#include <chrono>
#include <cctype>
#include <memory>
#include <thread>

using json = nlohmann::json;
constexpr int PROTOCOL_VERSION = 3;
constexpr size_t MAX_REQUEST_FRAME_BYTES = 16 * 1024 * 1024;
constexpr size_t MAX_RESPONSE_FRAME_BYTES = 1024 * 1024;

struct WhisperDeleter {
    void operator()(whisper_context * context) const { if (context) { whisper_free(context); } }
};
struct LoadedModel {
    std::string path;
    std::unique_ptr<whisper_context, WhisperDeleter> context;
    std::vector<float> pcm;
};

inline json worker_error(const std::string & request_id, const std::string & code, const std::string & message, const std::string & operation_id = "") {
    return {{"type", "error"}, {"protocol_version", PROTOCOL_VERSION}, {"request_id", request_id}, {"operation_id", operation_id.empty() ? json(nullptr) : json(operation_id)}, {"code", code}, {"message", message}};
}

inline bool ensure_model(const std::string & path, std::optional<LoadedModel> & loaded) {
    if (loaded && loaded->path == path) { return true; }
    if (path.empty()) { return false; }
    auto params = whisper_context_default_params();
    params.use_gpu = true;
    std::unique_ptr<whisper_context, WhisperDeleter> context(whisper_init_from_file_with_params(path.c_str(), params));
    if (!context) { return false; }
    loaded = LoadedModel{path, std::move(context), {}};
    return true;
}

inline std::string trim(const std::string & text) {
    size_t begin = 0, end = text.size();
    while (begin < end && std::isspace(static_cast<unsigned char>(text[begin]))) { ++begin; }
    while (end > begin && std::isspace(static_cast<unsigned char>(text[end - 1]))) { --end; }
    return text.substr(begin, end - begin);
}

inline json transcribe(const json & request, std::optional<LoadedModel> & loaded, std::atomic_bool & cancelled) {
    const auto id = request.value("request_id", "unparsed");
    const auto operation = request.value("operation_id", "");
    const auto timed = request.value("type", "") == "transcribe_window";
    const auto start = request.value("audio_start_sample", uint64_t{0});
    const auto language = request.value("language", "auto");
    const auto context = request.contains("context") && request["context"].is_string() ? request["context"].get<std::string>() : "";
    if (language.find('\0') != std::string::npos || context.find('\0') != std::string::npos) { return worker_error(id, "transcribe", "language/context contains a null byte", operation); }
    std::string audio_error;
    const auto samples = decode_pcm_i16_base64(request.value("samples_i16_base64", ""), audio_error);
    if (!samples) { return worker_error(id, "audio", audio_error, operation); }
    if (timed && samples->size() > 30 * 16000) { return worker_error(id, "audio", "window exceeds 30 seconds", operation); }
    if (cancelled.load()) { return worker_error(id, "cancelled", "request cancelled", operation); }
    if (!ensure_model(request.value("model_path", ""), loaded)) { return worker_error(id, "model_load", "could not load Whisper model", operation); }
    loaded->pcm.clear();
    for (const auto sample : *samples) { loaded->pcm.push_back(static_cast<float>(sample) / 32768.0f); }
    auto params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    params.n_threads = std::max(1u, std::min(8u, std::thread::hardware_concurrency()));
    params.no_context = true; params.single_segment = !timed;
    params.print_progress = false; params.print_realtime = false;
    params.print_timestamps = false; params.print_special = false;
    params.no_timestamps = !timed; params.token_timestamps = timed;
    params.language = language.empty() || language == "auto" ? nullptr : language.c_str();
    if (!context.empty()) { params.initial_prompt = context.c_str(); }
    params.abort_callback = [](void * signal) { return static_cast<std::atomic_bool *>(signal)->load(); };
    params.abort_callback_user_data = &cancelled;
    const auto started = std::chrono::steady_clock::now();
    const auto status = whisper_full(loaded->context.get(), params, loaded->pcm.data(), static_cast<int>(loaded->pcm.size()));
    if (cancelled.load()) { return worker_error(id, "cancelled", "request cancelled", operation); }
    if (status != 0) { return worker_error(id, "transcribe", "Whisper transcription failed", operation); }
    auto segments = json::array(), words = json::array();
    std::string text;
    const uint64_t end = start + samples->size();
    const auto absolute = [start, end](int64_t time) { return std::min(end, start + static_cast<uint64_t>(std::max(int64_t{0}, time)) * 160); };
    auto ctx = loaded->context.get();
    for (int i = 0; i < whisper_full_n_segments(ctx); ++i) {
        const char * raw = whisper_full_get_segment_text(ctx, i);
        if (!raw) { continue; }
        const auto segment = trim(raw);
        if (segment.empty()) { continue; }
        const uint64_t t0 = timed ? absolute(whisper_full_get_segment_t0(ctx, i)) : start;
        const uint64_t t1 = timed ? std::max(t0, absolute(whisper_full_get_segment_t1(ctx, i))) : end;
        segments.push_back({{"text", segment}, {"start_sample", t0}, {"end_sample", t1}});
        if (!text.empty()) { text += ' '; } text += segment;
        if (timed) {
            std::vector<TimedPiece> pieces;
            for (int j = 0; j < whisper_full_n_tokens(ctx, i); ++j) {
                const auto data = whisper_full_get_token_data(ctx, i, j);
                if (data.id >= whisper_token_eot(ctx)) { continue; }
                const char * bytes = whisper_full_get_token_text(ctx, i, j);
                if (bytes) { pieces.push_back({bytes, data.t0 >= 0 ? std::min(t1, std::max(t0, absolute(data.t0))) : t0, data.t1 >= 0 ? std::min(t1, absolute(data.t1)) : t1}); }
            }
            for (const auto & word : words_from_pieces(pieces)) { words.push_back(word); }
        }
    }
    const auto elapsed = std::chrono::duration<float>(std::chrono::steady_clock::now() - started).count();
    const auto audio_secs = static_cast<float>(samples->size()) / 16000.0f;
    if (!timed) { return {{"type", "result"}, {"protocol_version", PROTOCOL_VERSION}, {"request_id", id}, {"operation_id", operation}, {"text", text}, {"audio_secs", audio_secs}, {"transcribe_secs", elapsed}, {"backend", "vulkan"}}; }
    json detected = nullptr;
    if (language.empty() || language == "auto") { detected = whisper_lang_str(whisper_full_lang_id(ctx)); }
    return {{"type", "window_result"}, {"protocol_version", PROTOCOL_VERSION}, {"request_id", id}, {"operation_id", operation}, {"transcript", {{"text", text}, {"segments", segments}, {"words", words}, {"detected_language", detected}, {"transcribe_secs", elapsed}, {"audio_secs", audio_secs}, {"backend", "vulkan"}}}};
}

inline std::pair<json, bool> handle(const json & request, std::optional<LoadedModel> & loaded, std::atomic_bool & cancelled) {
    const auto id = request.value("request_id", "unparsed");
    const auto type = request.value("type", "");
    if (type == "hello") { return {{{"type", "ready"}, {"protocol_version", PROTOCOL_VERSION}, {"request_id", id}, {"backend", "vulkan"}, {"capabilities", {{"protocol_version", PROTOCOL_VERSION}, {"supports_health", true}, {"supports_shutdown", true}, {"supports_window", true}, {"supports_cancel", true}, {"supports_token_timestamps", true}, {"maximum_request_bytes", MAX_REQUEST_FRAME_BYTES}, {"maximum_response_bytes", MAX_RESPONSE_FRAME_BYTES}}}}, false}; }
    if (type == "ping") { return {{{"type", "pong"}, {"protocol_version", PROTOCOL_VERSION}, {"request_id", id}, {"backend", "vulkan"}}, false}; }
    if (type == "load") {
        if (!ensure_model(request.value("model_path", ""), loaded)) { return {worker_error(id, "model_load", "could not load Whisper model"), false}; }
        return {{{"type", "model_loaded"}, {"protocol_version", PROTOCOL_VERSION}, {"request_id", id}, {"backend", "vulkan"}}, false};
    }
    if (type == "transcribe" || type == "transcribe_window") { return {transcribe(request, loaded, cancelled), false}; }
    if (type == "shutdown") { loaded.reset(); return {{{"type", "shutting_down"}, {"protocol_version", PROTOCOL_VERSION}, {"request_id", id}}, true}; }
    return {worker_error(id, "request", "unsupported request type"), false};
}
