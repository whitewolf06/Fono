"""NumPy/ONNX inference only. Training tools and their imports stay offline."""
from __future__ import annotations
from functools import lru_cache
import candidate as common
import lira_ctc


@lru_cache(maxsize=1)
def frontend_constants():
    import numpy as np
    angles = np.arange(400, dtype=np.float32) * np.float32(2 * np.pi / 400)
    hann = (np.float32(1) - np.cos(angles)) / np.float32(2)
    return np.pad(hann, (56, 56)), common.mel_matrix()


def frontend(pcm):
    """The frozen 512/400/160 periodic-Hann log-mel transform, without Torch."""
    import numpy as np
    pcm = np.asarray(pcm, dtype=np.float32)
    if pcm.ndim != 2 or pcm.shape[1] != common.WINDOW or not np.isfinite(pcm).all():
        raise ValueError("frontend expects finite N x 64000 samples")
    window, mel = frontend_constants()
    frames = np.lib.stride_tricks.sliding_window_view(
        np.pad(pcm, ((0, 0), (352, 0))), 512, axis=1)[:, ::160]
    spectrum = np.fft.rfft(frames * window, axis=-1).astype(np.complex64)
    power = np.abs(spectrum).astype(np.float32) ** 2
    features = np.log(np.maximum(mel @ power.transpose(0, 2, 1), np.float32(1e-8)))
    mean = features.mean(axis=(1, 2), keepdims=True)
    std = np.maximum(features.std(axis=(1, 2), keepdims=True), np.float32(1))
    return ((features - mean) / std)[:, None].astype(np.float32)


@lru_cache(maxsize=1)
def half_rate_filter():
    import numpy as np
    taps = np.arange(-20, 21, dtype=np.float64)
    values = 0.5 * np.sinc(0.5 * taps) * np.kaiser(41, 5.0)
    return (values / values.sum()).astype(np.float32)


def resample_half(pcm):
    """Same symmetric Kaiser-5, 41-tap, zero-padded 16 -> 8 kHz polyphase FIR."""
    import numpy as np
    pcm = np.asarray(pcm, dtype=np.float32)
    if pcm.ndim != 1 or not len(pcm) or not np.isfinite(pcm).all():
        raise ValueError("finite mono PCM required")
    count = (len(pcm) + 1) // 2
    padded = np.pad(pcm, (20, 20))
    result = np.zeros(count, dtype=np.float32)
    # Accumulate in input order, like upfirdn, with float32 rounding per tap.
    for index, coefficient in enumerate(half_rate_filter()[::-1]):
        result += padded[index:index + 2 * count:2] * coefficient
    return result


def verify(tone, pcm):
    """The original exact greedy whole-phrase rule; no unused decoy DP scores."""
    return IncrementalVerifier(tone).advance(pcm, final=True)


class IncrementalVerifier:
    """Process a proposal's immutable prefix during its existing lookahead.

    Resampling needs 20 future input samples. Never commit that unfinished FIR
    edge; finalization supplies precisely the original zeros, padding and CTC
    rule. No early decisions, shorter context or reduced lookahead are used.
    """
    def __init__(self, tone):
        self.tone, self.np = tone, tone.np
        self.state = self.np.zeros((1, 219729), dtype=self.np.float16)
        self.frames, self.chunks, self.samples = [], 0, self.np.zeros(0, dtype=self.np.float32)
        self.finished = False

    def advance(self, pcm, *, final=False, max_chunks=2):
        np = self.np
        pcm = np.asarray(pcm, dtype=np.float32)
        if (self.finished or pcm.ndim != 1 or not 1 <= len(pcm) <= 90 * common.RATE
                or not np.isfinite(pcm).all() or len(pcm) < len(self.samples)
                or not np.array_equal(pcm[:len(self.samples)], self.samples)):
            raise ValueError("invalid or replaced verifier prefix")
        if not isinstance(max_chunks, int) or not 1 <= max_chunks <= 32:
            raise ValueError("invalid prefill budget")
        self.samples = pcm.copy()
        half = resample_half(pcm)
        if final:
            signal = np.pad(half, (2400, 4800))
            signal = np.pad(signal, (0, (-len(signal)) % 2400))
        else:
            # These output samples are identical to a later full resampling,
            # including float32 accumulation order and int32 quantization.
            stable = max(0, (len(pcm) - 20) // 2)
            signal = np.pad(half[:stable], (2400, 0))
        signal = (np.clip(signal, -1, 32767 / 32768) * 32768).astype(np.int32)
        available = len(signal) // 2400
        stop = available if final else min(available, self.chunks + max_chunks)
        while self.chunks < stop:
            chunk = signal[self.chunks * 2400:(self.chunks + 1) * 2400]
            logp, state = self.tone.session.run(None, {
                "signal": chunk.reshape(1, 2400, 1), "state": self.state,
            })
            if logp.shape != (1, 10, 35) or state.shape != (1, 219729):
                raise ValueError("invalid streaming T-one output")
            self.state = state
            self.frames.extend(logp[0].tolist())
            self.chunks += 1
        if not final:
            return None
        self.finished = True
        return lira_ctc.whole_phrase(lira_ctc.greedy_text(self.frames, self.tone.vocabulary, self.tone.blank))
