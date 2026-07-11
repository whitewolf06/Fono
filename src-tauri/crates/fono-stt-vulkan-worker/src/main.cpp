#include "whisper.h"
#include "json.hpp"

#include <algorithm>
#include <array>
#include <chrono>
#include <cctype>
#include <cstdint>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <thread>
#include <vector>

using json = nlohmann::json;

namespace {

struct WhisperDeleter {
    void operator()(whisper_context * context) const {
        if (context != nullptr) {
            whisper_free(context);
        }
    }
};

using WhisperContextPtr = std::unique_ptr<whisper_context, WhisperDeleter>;

struct LoadedModel {
    std::string path;
    WhisperContextPtr context{nullptr};
};

std::string trim(std::string value) {
    const auto first = std::find_if_not(value.begin(), value.end(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    });
    const auto last = std::find_if_not(value.rbegin(), value.rend(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    }).base();
    return first < last ? std::string(first, last) : std::string();
}

std::optional<std::vector<int16_t>> decode_pcm_i16_base64(const std::string & encoded, std::string & error) {
    static const auto decode_table = [] {
        std::array<int8_t, 256> table{};
        table.fill(-1);
        for (int index = 0; index < 26; ++index) {
            table[static_cast<unsigned char>('A' + index)] = static_cast<int8_t>(index);
            table[static_cast<unsigned char>('a' + index)] = static_cast<int8_t>(26 + index);
        }
        for (int index = 0; index < 10; ++index) {
            table[static_cast<unsigned char>('0' + index)] = static_cast<int8_t>(52 + index);
        }
        table[static_cast<unsigned char>('+')] = 62;
        table[static_cast<unsigned char>('/')] = 63;
        return table;
    }();

    if (encoded.empty() || encoded.size() % 4 != 0) {
        error = "PCM payload is not valid base64";
        return std::nullopt;
    }

    std::vector<uint8_t> bytes;
    bytes.reserve(encoded.size() / 4 * 3);
    for (size_t offset = 0; offset < encoded.size(); offset += 4) {
        const char c0 = encoded[offset];
        const char c1 = encoded[offset + 1];
        const char c2 = encoded[offset + 2];
        const char c3 = encoded[offset + 3];
        const bool pad2 = c2 == '=';
        const bool pad3 = c3 == '=';
        if (pad2 && !pad3) {
            error = "PCM payload has invalid base64 padding";
            return std::nullopt;
        }
        const int8_t a = decode_table[static_cast<unsigned char>(c0)];
        const int8_t b = decode_table[static_cast<unsigned char>(c1)];
        const int8_t c = pad2 ? 0 : decode_table[static_cast<unsigned char>(c2)];
        const int8_t d = pad3 ? 0 : decode_table[static_cast<unsigned char>(c3)];
        if (a < 0 || b < 0 || c < 0 || d < 0 || ((pad2 || pad3) && offset + 4 != encoded.size())) {
            error = "PCM payload is not valid base64";
            return std::nullopt;
        }
        bytes.push_back(static_cast<uint8_t>((a << 2) | (b >> 4)));
        if (!pad2) {
            bytes.push_back(static_cast<uint8_t>((b << 4) | (c >> 2)));
        }
        if (!pad3) {
            bytes.push_back(static_cast<uint8_t>((c << 6) | d));
        }
    }
    if (bytes.size() % 2 != 0) {
        error = "PCM payload has an odd byte length";
        return std::nullopt;
    }

    std::vector<int16_t> samples;
    samples.reserve(bytes.size() / 2);
    for (size_t index = 0; index < bytes.size(); index += 2) {
        const uint16_t value = static_cast<uint16_t>(bytes[index]) |
            (static_cast<uint16_t>(bytes[index + 1]) << 8);
        samples.push_back(static_cast<int16_t>(value));
    }
    return samples;
}

json worker_error(const std::optional<std::string> & id, const std::string & code, const std::string & message) {
    json response = {
        {"type", "error"},
        {"code", code},
        {"message", message},
    };
    if (id.has_value()) {
        response["id"] = *id;
    } else {
        response["id"] = nullptr;
    }
    return response;
}

WhisperContextPtr load_model(const std::string & path) {
    whisper_context_params params = whisper_context_default_params();
    params.use_gpu = true;
    return WhisperContextPtr(whisper_init_from_file_with_params(path.c_str(), params));
}

bool ensure_model(const std::string & model_path, std::optional<LoadedModel> & loaded, std::string & error) {
    if (model_path.empty()) {
        error = "model_path is required";
        return false;
    }
    if (loaded.has_value() && loaded->path == model_path) {
        return true;
    }
    auto context = load_model(model_path);
    if (context == nullptr) {
        error = "could not load Whisper model";
        return false;
    }
    loaded = LoadedModel{model_path, std::move(context)};
    return true;
}

json transcribe(const json & request, std::optional<LoadedModel> & loaded) {
    const auto id = request.value("id", "");
    const auto model_path = request.value("model_path", "");
    const auto language = request.value("language", "auto");
    const auto encoded = request.value("samples_i16_base64", "");
    if (id.empty() || model_path.empty()) {
        return worker_error(id.empty() ? std::nullopt : std::make_optional(id), "request", "id and model_path are required");
    }

    std::string audio_error;
    const auto samples = decode_pcm_i16_base64(encoded, audio_error);
    if (!samples.has_value()) {
        return worker_error(id, "audio", audio_error);
    }

    std::string model_error;
    if (!ensure_model(model_path, loaded, model_error)) {
        return worker_error(id, "model_load", model_error);
    }

    std::vector<float> pcm;
    pcm.reserve(samples->size());
    for (const auto sample : *samples) {
        pcm.push_back(static_cast<float>(sample) / 32768.0f);
    }

    auto params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    const auto hardware_threads = std::thread::hardware_concurrency();
    params.n_threads = static_cast<int>(std::max(1u, std::min(8u, hardware_threads)));
    params.translate = false;
    params.no_context = true;
    params.single_segment = true;
    params.print_progress = false;
    params.print_realtime = false;
    params.print_timestamps = false;
    params.print_special = false;
    params.no_timestamps = true;
    if (!language.empty() && language != "auto") {
        params.language = language.c_str();
    }

    const auto started = std::chrono::steady_clock::now();
    if (whisper_full(loaded->context.get(), params, pcm.data(), static_cast<int>(pcm.size())) != 0) {
        return worker_error(id, "transcribe", "Whisper transcription failed");
    }

    std::string text;
    const int segments = whisper_full_n_segments(loaded->context.get());
    for (int index = 0; index < segments; ++index) {
        const char * segment = whisper_full_get_segment_text(loaded->context.get(), index);
        if (segment == nullptr) {
            continue;
        }
        const auto normalized = trim(segment);
        if (!normalized.empty()) {
            if (!text.empty()) {
                text += ' ';
            }
            text += normalized;
        }
    }

    const auto elapsed = std::chrono::duration<float>(std::chrono::steady_clock::now() - started).count();
    return {
        {"type", "result"},
        {"id", id},
        {"text", text},
        {"audio_secs", static_cast<float>(samples->size()) / 16000.0f},
        {"transcribe_secs", elapsed},
        {"backend", "vulkan"},
    };
}

json handle_line(const std::string & line, std::optional<LoadedModel> & loaded) {
    try {
        const auto request = json::parse(line);
        const auto type = request.value("type", "");
        if (type == "ping") {
            return {{"type", "ready"}, {"backend", "vulkan"}};
        }
        if (type == "load") {
            std::string model_error;
            if (!ensure_model(request.value("model_path", ""), loaded, model_error)) {
                return worker_error(std::nullopt, "model_load", model_error);
            }
            return {{"type", "model_loaded"}, {"backend", "vulkan"}};
        }
        if (type == "transcribe") {
            return transcribe(request, loaded);
        }
        return worker_error(std::nullopt, "request", "unsupported request type");
    } catch (const std::exception & error) {
        return worker_error(std::nullopt, "request_json", error.what());
    }
}

} // namespace

int main() {
    std::ios::sync_with_stdio(false);
    std::optional<LoadedModel> loaded;
    std::string line;
    while (std::getline(std::cin, line)) {
        const auto response = handle_line(line, loaded);
        std::cout << response.dump() << '\n' << std::flush;
    }
    return 0;
}
