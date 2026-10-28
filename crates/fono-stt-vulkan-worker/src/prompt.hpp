#pragma once
#include "whisper.h"
#include "json.hpp"
#include <optional>
#include <string>
#include <vector>

inline bool valid_glossary(const nlohmann::json & terms) {
    if (!terms.is_array() || terms.size() > 32) { return false; }
    size_t bytes = 0;
    for (const auto & value : terms) {
        if (!value.is_string()) { return false; }
        const auto term = value.get<std::string>();
        if (term.find_first_not_of(" \t") == std::string::npos || term.find('\0') != std::string::npos || term.find_first_of("\r\n") != std::string::npos) { return false; }
        bytes += term.size();
    }
    return bytes + (terms.empty() ? 0 : (terms.size() - 1) * 2) <= 896;
}

// Same bounds/order as fono_stt_protocol::prompt; owned until whisper_full returns.
inline std::optional<std::vector<whisper_token>> prompt_tokens(whisper_context * ctx, const nlohmann::json & terms, const std::string & context) {
    if (!valid_glossary(terms)) { return std::nullopt; }
    const auto tokenize = [ctx](const std::string & text) -> std::optional<std::vector<whisper_token>> {
        const int required = whisper_tokenize(ctx, text.c_str(), nullptr, 0);
        if (required == 0) { return std::vector<whisper_token>{}; }
        if (required > 0 || required < -65536) { return std::nullopt; }
        std::vector<whisper_token> tokens(static_cast<size_t>(-required));
        const int count = whisper_tokenize(ctx, text.c_str(), tokens.data(), static_cast<int>(tokens.size()));
        if (count < 0) { return std::nullopt; }
        tokens.resize(static_cast<size_t>(count));
        return tokens;
    };
    std::string accepted;
    std::vector<whisper_token> tokens;
    for (const auto & value : terms) {
        const auto term = value.get<std::string>();
        const auto candidate = accepted.empty() ? term : accepted + ", " + term;
        auto candidate_tokens = tokenize(candidate);
        if (!candidate_tokens) { return std::nullopt; }
        if (candidate_tokens->size() <= 224) { accepted = candidate; tokens = std::move(*candidate_tokens); }
    }
    if (!context.empty()) {
        auto candidate_tokens = tokenize(accepted.empty() ? context : accepted + ". " + context);
        if (!candidate_tokens) { return std::nullopt; }
        if (candidate_tokens->size() <= 224) { tokens = std::move(*candidate_tokens); }
    }
    return tokens;
}
