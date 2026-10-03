#pragma once
#include <algorithm>
#include <cstdint>
#include <stdexcept>
#include <string>
#include <vector>
#include "json.hpp"

struct TimedPiece {
    std::string bytes;
    uint64_t start;
    uint64_t end;
};

inline uint32_t next_utf8(const std::string & text, size_t & index) {
    const auto first = static_cast<unsigned char>(text.at(index++));
    if (first < 128) { return first; }
    int remaining = first >= 0xc2 && first <= 0xdf ? 1 : first >= 0xe0 && first <= 0xef ? 2 : first >= 0xf0 && first <= 0xf4 ? 3 : -1;
    if (remaining < 0 || index + remaining > text.size()) { throw std::runtime_error("invalid UTF-8 hypothesis"); }
    uint32_t value = first & (remaining == 1 ? 31 : remaining == 2 ? 15 : 7);
    const int total = remaining;
    while (remaining-- > 0) {
        const auto next = static_cast<unsigned char>(text[index++]);
        if ((next & 0xc0) != 0x80) { throw std::runtime_error("invalid UTF-8 continuation"); }
        value = (value << 6) | (next & 63);
    }
    if ((total == 1 && value < 128) || (total == 2 && value < 2048) || (total == 3 && value < 65536)
        || value > 0x10ffff || (value >= 0xd800 && value <= 0xdfff)) { throw std::runtime_error("invalid UTF-8 scalar"); }
    return value;
}

inline bool word_space(uint32_t value) {
    return (value >= 9 && value <= 13) || value == 32 || value == 0x85 || value == 0xa0
        || value == 0x1680 || (value >= 0x2000 && value <= 0x200a)
        || value == 0x2028 || value == 0x2029 || value == 0x202f || value == 0x205f || value == 0x3000;
}

inline nlohmann::json words_from_pieces(const std::vector<TimedPiece> & pieces) {
    std::string text;
    for (const auto & piece : pieces) { text += piece.bytes; }
    auto result = nlohmann::json::array();
    size_t start = std::string::npos;
    size_t index = 0;
    while (index <= text.size()) {
        const size_t before = index;
        const auto scalar = index == text.size() ? (index++, 32u) : next_utf8(text, index);
        if (!word_space(scalar)) { if (start == std::string::npos) { start = before; } continue; }
        if (start == std::string::npos) { continue; }
        size_t offset = 0;
        bool found = false;
        uint64_t t0 = 0, t1 = 0;
        for (const auto & piece : pieces) {
            const size_t end = offset + piece.bytes.size();
            if (offset < before && end > start) {
                if (!found) { t0 = piece.start; found = true; }
                t1 = piece.end;
            }
            offset = end;
        }
        if (found) { result.push_back({{"text", text.substr(start, before - start)}, {"start_sample", t0}, {"end_sample", std::max(t0, t1)}}); }
        start = std::string::npos;
    }
    return result;
}
