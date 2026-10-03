#include "timestamps.hpp"
#include "pcm.hpp"
#include <cassert>

int main() {
    const std::string text = u8" да, да музыка";
    std::vector<TimedPiece> pieces;
    for (size_t i = 0; i < text.size(); ++i) { pieces.push_back({text.substr(i, 1), 100 + i, 101 + i}); }
    const auto words = words_from_pieces(pieces);
    assert(words.size() == 3);
    assert(words[0]["text"] == u8"да,");
    assert(words[1]["text"] == u8"да");
    assert(words[2]["text"] == u8"музыка");
    assert(words[0]["start_sample"] == 101);
    bool rejected = false;
    try { words_from_pieces({{std::string(1, '\xff'), 0, 1}}); }
    catch (const std::runtime_error &) { rejected = true; }
    assert(rejected);
    std::string error;
    assert(!decode_pcm_i16_base64("", error));
    assert(!decode_pcm_i16_base64("AQ==", error));
    assert(!decode_pcm_i16_base64("AAB=", error));
    const auto samples = decode_pcm_i16_base64("AIA=", error);
    assert(samples && samples->size() == 1 && (*samples)[0] == -32768);
}
