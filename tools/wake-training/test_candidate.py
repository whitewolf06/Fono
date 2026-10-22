"""No models/frameworks/microphone: provenance and causal replay contract tests."""
import json
from pathlib import Path
import struct
import tempfile
import unittest
import wave

import candidate
import synthesize_candidate


class CandidateTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="fono-wake-candidate-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def wav(self, path, value=1):
        with wave.open(str(path), "wb") as audio:
            audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            audio.writeframes(struct.pack("<h", value) * 1600)

    def manifest(self):
        entries = []
        for language in ("ru", "en"):
            for positive in (False, True):
                path = self.root / f"{language}-{positive}.wav"
                self.wav(path)
                entries.append({"wav": path.name, "sha256": candidate.digest(path),
                                "samples": 1600, "language": language, "positive": positive,
                                "origin": "synthetic", "split": "train"})
        return {"schema": "fono-wake-synthetic-candidate@1", "entries": entries,
                "owner_voice_used": False, "reference_audio_used": False}

    def test_synthesis_jobs_have_only_public_designs_no_reference_audio(self):
        jobs = synthesize_candidate.jobs()
        self.assertEqual(len(jobs), 96)
        self.assertEqual(sum(j["positive"] for j in jobs), 24)
        self.assertEqual(len({j["seed"] for j in jobs}), 96)
        self.assertEqual({j["language"] for j in jobs}, {"ru", "en"})
        self.assertTrue(all(set(j) == {"language", "positive", "text", "style", "seed", "speaker"} for j in jobs))

    def test_fixed_point_edge_and_cooldown_prevent_retrigger_of_continuous_score(self):
        self.assertEqual(candidate.events([0.9, 0.9, 0.9], [1280, 64000, 128000]), [1280])
        self.assertEqual(candidate.events([0.9, 0.1, 0.9], [1280, 2560, 64000]), [1280, 64000])
        self.assertEqual(candidate.events([0.9, 0.1, 0.9], [1280, 2560, 3840]), [1280])

    def test_nonfinite_score_and_mismatched_clocks_are_rejected(self):
        for score in (float("nan"), float("inf"), -0.1, 1.1):
            with self.assertRaises(ValueError):
                candidate.events([score], [1280])
        with self.assertRaises(ValueError):
            candidate.events([0.9], [])

    def test_train_provenance_rejects_owner_dev_and_paths(self):
        path = self.root / "training.json"
        for field, value in (("split", "dev"), ("split", "test"), ("origin", "real"),
                             ("wav", "../outside.wav"), ("wav", "..\\outside.wav")):
            manifest = self.manifest()
            manifest["entries"][0][field] = value
            candidate.write_json(path, manifest)
            with self.assertRaises(ValueError):
                candidate.training_entries(path)

    def test_explicit_no_owner_voice_is_required(self):
        path = self.root / "training.json"
        for field in ("owner_voice_used", "reference_audio_used"):
            manifest = self.manifest()
            manifest[field] = True
            candidate.write_json(path, manifest)
            with self.assertRaises(ValueError):
                candidate.training_entries(path)

    def test_pcm_hash_corruption_is_not_replaced_by_silence(self):
        manifest = self.manifest()
        path = self.root / "training.json"
        candidate.write_json(path, manifest)
        self.wav(self.root / manifest["entries"][0]["wav"], value=2)
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            candidate.training_entries(path)


if __name__ == "__main__":
    unittest.main()
