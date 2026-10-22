"""The manual bridge rejects invalid audio before loading an inference runtime."""
import io
import unittest
import wave
import dev_check


def wav(rate=16000, channels=1, frames=4000):
    out = io.BytesIO()
    with wave.open(out, "wb") as audio:
        audio.setnchannels(channels)
        audio.setframerate(rate)
        audio.setsampwidth(2)
        audio.writeframes(b"\0\0" * channels * frames)
    return out.getvalue()


class InputTests(unittest.TestCase):
    def test_only_bounded_complete_pcm_is_admitted(self):
        self.assertEqual(len(dev_check.read_input(io.BytesIO(wav()))), 8000)
        for raw in (wav(rate=8000), wav(channels=2), wav(frames=10),
                    wav(frames=4000)[:-2], b"\0" * (dev_check.MAX_WAV_BYTES + 1)):
            with self.assertRaises((ValueError, wave.Error)):
                dev_check.read_input(io.BytesIO(raw))


if __name__ == "__main__":
    unittest.main()
