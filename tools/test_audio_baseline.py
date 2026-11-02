"""Signal diagnostics must expose alignment/gain and undefined silent cases."""
import math
import subprocess
import sys
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

    def test_packet_budgets_include_rounding_and_dtx_exceptions(self):
        encoded = [b'x'*n for n in [80, 81, 80, 80]]
        baseline.validate_packet_budgets(encoded, 32101, 960, False)
        baseline.validate_packet_budgets([b'x'*80, b'x', b'x'*80, b'x'*80], 32101, 960, True)
        for packets, dtx in [([b'x'], False), ([b'xx'], True), ([b'x'*80]*4, False)]:
            with self.assertRaises(ValueError):
                baseline.validate_packet_budgets(packets, 32101, 960, dtx)

    def test_plc_diagnostics_cannot_hide_nonzero_or_failed_processes(self):
        command = [sys.executable, '-c', "import sys; print('encoded 10 PLC fallback packets',file=sys.stderr)"]
        self.assertEqual(baseline.encode_diagnostic(command, 'silk'), 10)
        with self.assertRaises(ValueError):
            baseline.encode_diagnostic([sys.executable, '-c', 'pass'], 'silk')
        with self.assertRaises(subprocess.CalledProcessError):
            baseline.encode_diagnostic([sys.executable, '-c', 'raise SystemExit(2)'], 'silk')


if __name__ == '__main__':
    unittest.main()
