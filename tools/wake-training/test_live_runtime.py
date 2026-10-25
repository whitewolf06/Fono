"""Numerical contracts against the frozen training frontend and resampler."""
import unittest
import numpy as np
import candidate as common
import live_runtime as runtime


class RuntimeTests(unittest.TestCase):
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
