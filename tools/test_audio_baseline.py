"""Signal diagnostics must expose alignment/gain and undefined silent cases."""
import math
from pathlib import Path
import tempfile
import unittest

import audio_baseline as baseline


class SignalDiagnosticsTests(unittest.TestCase):
    def test_known_gain_error_and_phase_reversal_are_visible(self):
        original = [-100, 100, -200, 200]
        half = baseline.signal_metrics(original, [-50, 50, -100, 100])
        self.assertAlmostEqual(half['snr_db'], 10*math.log10(4))
        self.assertAlmostEqual(half['correlation'], 1)
        self.assertEqual(half['max_abs_error'], 100)
        self.assertEqual(half['mean_error'], 0)
        reverse = baseline.signal_metrics(original, [-x for x in original])
        self.assertAlmostEqual(reverse['snr_db'], -10*math.log10(4))
        self.assertAlmostEqual(reverse['correlation'], -1)

    def test_identical_silent_muted_and_clipped_are_distinct(self):
        silence = baseline.signal_metrics([0, 0], [0, 0])
        self.assertTrue(silence['identical'])
        self.assertIsNone(silence['snr_db'])
        self.assertIsNone(silence['correlation'])
        mute = baseline.signal_metrics([-1, 1], [0, 0])
        self.assertFalse(mute['identical'])
        self.assertEqual(mute['snr_db'], 0)
        self.assertIsNone(mute['correlation'])
        clipped = baseline.signal_metrics([0, 0], [-32768, 32767])
        self.assertEqual(clipped['peak'], 32768)
        self.assertEqual(clipped['clipped_samples'], 2)
        for source, decoded in [([], []), ([0], []), ([0], [0, 0])]:
            with self.assertRaises(ValueError):
                baseline.signal_metrics(source, decoded)

    def test_wave_extremes_and_original_ogg_extraction(self):
        root = Path(__file__).resolve().parent.parent
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/'stereo.wav'
            baseline.write_wave(path, 2, [-32768, 32767, 0, -1])
            channels, values = baseline.read_wave(path)
            self.assertEqual(channels, 2)
            self.assertEqual(list(values), [-32768, 32767, 0, -1])
            original = (root/'tests/audio/celt-stereo.opus').read_bytes()
            path = Path(temp)/'file.opus'
            path.write_bytes(original)
            skip, packets = baseline.packets(path)
            self.assertEqual(skip, 120)
            self.assertEqual(len(packets), 5)
            for data in [b'', original[:28], original[:-1]]:
                path.write_bytes(data)
                with self.assertRaises(ValueError):
                    baseline.packets(path)


if __name__ == '__main__':
    unittest.main()
