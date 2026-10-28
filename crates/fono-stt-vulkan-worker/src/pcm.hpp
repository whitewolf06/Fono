#pragma once
#include <array>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>
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
        if ((pad2 && (b & 15) != 0) || (pad3 && !pad2 && (c & 3) != 0)) {
            error = "PCM payload has non-canonical base64 padding bits";
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
