"""Lira provenance contracts without models, microphone or network."""
import json
from pathlib import Path
import tempfile
import unittest

import lira
import synthesize_candidate


class LiraTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="fono-lira-contract-")
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name) / "training.json"

    def manifest(self):
        return {"schema": "fono-wake-synthetic-candidate@1", "phrase_id": "privet_lira",
                "target_phrases": {"ru": "Привет, Лира"}, "owner_voice_used": False,
                "reference_audio_used": False,
                "entries": [{"wav": "unread.wav", "origin": "synthetic", "split": "train",
                             "language": "ru", "positive": True, "text": "Привет, Лира."}]}

    def reject(self, data):
        self.path.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaises(ValueError):
            lira.training_sources(self.path)

    def test_synthesis_keeps_target_and_hard_negatives_separate_across_voice_designs(self):
        jobs = synthesize_candidate.jobs("privet_lira")
        positives = [j for j in jobs if j["positive"]]
        negatives = [j for j in jobs if not j["positive"]]
        self.assertEqual(len(jobs), 36)
        self.assertEqual(len(positives), 8)
        self.assertEqual({lira.normalized(j["text"]) for j in positives}, {"привет лира"})
        self.assertEqual({j["speaker"] for j in jobs[:4]}, {j["speaker"] for j in jobs})
        self.assertTrue(any("айфон" in j["text"] for j in negatives))
        self.assertTrue(any("Лера" in j["text"] for j in negatives))
        self.assertFalse(any("привет лира" in lira.normalized(j["text"]) for j in negatives))
        with self.assertRaises(ValueError):
            synthesize_candidate.jobs("unknown")

    def test_legacy_phrase_owner_voice_and_reference_voice_cannot_be_training_fallbacks(self):
        for key, value in (("phrase_id", "hey_fono"), ("owner_voice_used", True),
                           ("reference_audio_used", True), ("target_phrases", {"en": "Hello Lyra"})):
            data = self.manifest()
            data[key] = value
            self.reject(data)

    def test_private_splits_paths_and_wrong_target_are_rejected_before_audio_io(self):
        for key, value in (("split", "dev"), ("split", "test"), ("origin", "real"),
                           ("wav", "../private.wav"), ("wav", "..\\private.wav"),
                           ("language", "en"), ("text", "Эй, Фоно."), ("positive", False)):
            data = self.manifest()
            data["entries"][0][key] = value
            self.reject(data)

    def test_changed_public_selection_is_rejected_without_loading_frameworks(self):
        self.path.write_text('{"entries": []}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "public training selection changed"):
            lira.public_sources(self.path)


if __name__ == "__main__":
    unittest.main()
