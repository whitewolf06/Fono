#pragma once
#include "json.hpp"
#include <optional>
#include <string>
#include <thread>
#include <algorithm>

struct DecodingOptions {
    bool accurate = false;
    int threads = static_cast<int>(std::max(1u, std::min(8u, std::thread::hardware_concurrency())));
};

inline std::optional<DecodingOptions> decoding_options(const nlohmann::json & request) {
    DecodingOptions options;
    if (!request.contains("decoding")) { return options; }
    const auto & value = request["decoding"];
    if (!value.is_object()) { return std::nullopt; }
    for (const auto & item : value.items()) {
        if (item.key() != "strategy" && item.key() != "threads") { return std::nullopt; }
    }
    if (value.contains("strategy")) {
        if (!value["strategy"].is_string()) { return std::nullopt; }
        const auto strategy = value["strategy"].get<std::string>();
        if (strategy != "fast" && strategy != "accurate") { return std::nullopt; }
        options.accurate = strategy == "accurate";
    }
    if (value.contains("threads") && !value["threads"].is_null()) {
        if (!value["threads"].is_number_integer()) { return std::nullopt; }
        const auto threads = value["threads"].get<int64_t>();
        if (threads < 1 || threads > 8) { return std::nullopt; }
        options.threads = static_cast<int>(threads);
    }
    return options;
}
