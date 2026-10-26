"""Numerical contracts against the frozen training frontend and resampler."""
import unittest
from types import SimpleNamespace
import numpy as np
import candidate as common
import live_runtime as runtime


class RuntimeTests(unittest.TestCase):
    def test_prefill_preserves_every_frozen_input_chunk_including_resampler_edges(self):
        class Session:
            def __init__(self):
                self.chunks = []
            def run(self, _, inputs):
                self.chunks.append(inputs["signal"].copy())
                # A stateful stand-in also detects losing/resetting model state.
                self.assert_state(inputs["state"], len(self.chunks) - 1)
                state = inputs["state"].copy()
                state[0, 0] += 1
                return np.zeros((1, 10, 35), np.float32), state
            @staticmethod
            def assert_state(state, expected):
                if state[0, 0] != expected:
                    raise AssertionError("streaming state was not retained")
        rng = np.random.default_rng(20261026)
        for length in (1280 + common.TAIL, common.WINDOW + common.TAIL, common.WINDOW + common.TAIL - 1):
            pcm = rng.uniform(-.7, .7, length).astype(np.float32)
            # High contrast at FIR edges catches committing zero-padded samples
            # before the 20-sample right halo is actually available.
            pcm[6380:6421] = 0
            pcm[6400] = .99
            expected = np.pad(runtime.resample_half(pcm), (2400, 4800))
            expected = np.pad(expected, (0, (-len(expected)) % 2400))
            expected = (np.clip(expected, -1, 32767 / 32768) * 32768).astype(np.int32)
            session = Session()
            tone = SimpleNamespace(np=np, session=session, vocabulary={"":0}, blank=0)
            verifier = runtime.IncrementalVerifier(tone)
            for end in range(max(1, length - common.TAIL), length, common.STRIDE):
                before = len(session.chunks)
                self.assertIsNone(verifier.advance(pcm[:end]))
                self.assertLessEqual(len(session.chunks) - before, 2)
            self.assertFalse(verifier.advance(pcm, final=True))
            actual = np.concatenate(session.chunks).reshape(-1)
            np.testing.assert_array_equal(actual, expected)
            with self.assertRaises(ValueError): verifier.advance(pcm, final=True)

    def test_prefill_rejects_replaced_truncated_or_nonfinite_audio_before_inference(self):
        tone = SimpleNamespace(np=np, session=SimpleNamespace(run=lambda _, inputs:
            (np.zeros((1, 10, 35), np.float32), inputs["state"])))
        verifier = runtime.IncrementalVerifier(tone)
        # Only the fixed left padding can reach the stand-in model here.
        pcm = np.ones(1, np.float32)
        self.assertIsNone(verifier.advance(pcm))
        for bad in (np.zeros(0), np.zeros(2), np.array([np.nan]), np.ones((1, 2))):
            with self.assertRaises(ValueError): verifier.advance(bad)

    def test_logmel_matches_the_frozen_training_frontend(self):
        import torch
        torch.set_num_threads(1)
        rng = np.random.default_rng(20261026)
        pcm = np.stack((np.zeros(common.WINDOW),
                        rng.normal(0, .03, common.WINDOW),
                        np.sin(np.arange(common.WINDOW) * .07) * .2)).astype(np.float32)
        expected = common.frontend(pcm).numpy()
        actual = runtime.frontend(pcm)
        self.assertEqual(actual.shape, expected.shape)
        self.assertEqual(actual.dtype, np.float32)
        # Independent float32 FFT/cos implementations differ near the log floor.
        # The paired replay separately requires CNN-score parity and identical
        # decisions/sample clocks; this bound concerns features, not acceptance.
        np.testing.assert_allclose(actual, expected, rtol=2e-4, atol=1e-3)

    def test_polyphase_resampling_matches_frozen_scipy_with_odd_lengths_and_edges(self):
        from scipy.signal import resample_poly
        rng = np.random.default_rng(20261026)
        for length in (1, 2, 17, 4000, 16001, 74240):
            pcm = rng.uniform(-.5, .5, length).astype(np.float32)
            expected = resample_poly(pcm, 1, 2, window=("kaiser", 5.0))
            actual = runtime.resample_half(pcm)
            np.testing.assert_allclose(actual, expected, rtol=1e-6, atol=1e-7)
        for position in (0, 20, 42):
            pcm = np.zeros(43, dtype=np.float32)
            pcm[position] = 1
            np.testing.assert_array_equal(runtime.resample_half(pcm), resample_poly(pcm, 1, 2))

    def test_invalid_audio_never_reaches_fft_or_verifier(self):
        for value in (np.zeros((1, 1)), np.full((1, common.WINDOW), np.nan)):
            with self.assertRaises(ValueError): runtime.frontend(value)
        for value in (np.zeros(0), np.full(10, np.inf), np.zeros((2, 10))):
            with self.assertRaises(ValueError): runtime.resample_half(value)


if __name__ == "__main__":
    unittest.main()
