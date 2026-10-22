"""Model-free CTC alignment and nonprivate calibration contracts."""
import math
import json
from pathlib import Path
import unittest

import lira_ctc


def frames(sequence):
    return [[0.0 if i == token else -9.0 for i in range(4)] for token in sequence]


class LiraCtcTests(unittest.TestCase):
    def test_reviewed_point_keeps_exact_bytes_source_and_asset_identity(self):
        path = Path(lira_ctc.__file__).with_name("lira-ctc-exact") / "config.json"
        self.assertEqual(lira_ctc.digest_file(path), "d3ea4079b329755bcdd42618cec915aa4952554479eb4b0999689558305ee47e")
        point = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(point["source_sha256"], lira_ctc.digest_file(lira_ctc.__file__))
        self.assertEqual(point["model_identity"], lira_ctc.FILE_SHA256)
        self.assertEqual(point["decision_rule"], "greedy_whole_phrase")
        self.assertIsNone(point["cutoff"])
        self.assertIs(point["private_voice_used_for_calibration"], False)

    def test_exact_decoder_requires_whole_words_and_preserves_ctc_repeated_letters(self):
        self.assertTrue(lira_ctc.whole_phrase("ну ПРИВЕТ ЛИРА идём"))
        for text in ("привет лера", "привет мира", "приветлира", "непривет лира", "привет лирами"):
            self.assertFalse(lira_ctc.whole_phrase(text))
        vocabulary = {"<blk>": 0, "а": 1, "б": 2, " ": 3}
        self.assertEqual(lira_ctc.greedy_text(frames([1, 1, 0, 1, 3, 2, 2]), vocabulary, 0), "аа б")

    def test_complete_target_can_be_inside_unrelated_speech_but_order_matters(self):
        emissions = frames([3, 0, 1, 0, 2, 0, 3])
        self.assertEqual(lira_ctc.spot_score(emissions, [1, 2], 0), 0.0)
        self.assertLess(lira_ctc.spot_score(emissions, [2, 1], 0), -1.0)
        self.assertLess(lira_ctc.spot_score(emissions, [1, 2, 1], 0), -1.0)

    def test_repeated_character_needs_a_ctc_blank_instead_of_one_stretched_sound(self):
        self.assertEqual(lira_ctc.spot_score(frames([1, 1]), [1, 1], 0), -math.inf)
        self.assertEqual(lira_ctc.spot_score(frames([1, 0, 1]), [1, 1], 0), 0.0)

    def test_cutoff_uses_nonprivate_separation_and_decoy_cannot_override_target_margin(self):
        rows = [{"positive": True, "target_score": -0.2, "margin": 0.1},
                {"positive": True, "target_score": -0.1, "margin": 0.3},
                {"positive": False, "target_score": -0.8, "margin": 0.1},
                {"positive": False, "target_score": -0.01, "margin": -0.1}]
        cutoff = lira_ctc.choose_cutoff(rows)
        self.assertEqual([lira_ctc.accepted(r, cutoff) for r in rows], [True, True, False, False])
        self.assertFalse(lira_ctc.accepted({"target_score": 0, "margin": 0}, cutoff))

    def test_overlapping_calibration_fails_instead_of_using_private_threshold_tuning(self):
        with self.assertRaisesRegex(ValueError, "no separating cutoff"):
            lira_ctc.choose_cutoff([{"positive": True, "target_score": -0.8, "margin": 1},
                                    {"positive": False, "target_score": -0.1, "margin": 1}])

    def test_invalid_emissions_or_tokens_are_not_treated_as_matching_silence(self):
        for emissions, tokens, blank in [([], [1], 0), (frames([1]), [], 0),
                                          (frames([1]), [0], 0), (frames([1]), [4], 0),
                                          ([[0, float("nan"), -1, -1]], [1], 0),
                                          ([[0, -1], [0]], [1], 0)]:
            with self.assertRaises(ValueError):
                lira_ctc.spot_score(emissions, tokens, blank)


if __name__ == "__main__":
    unittest.main()
