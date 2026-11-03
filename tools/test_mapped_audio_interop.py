"""Adversarial checks for the optional independent file evidence reader."""
from pathlib import Path
import struct
import unittest
from mapped_audio_fixtures import crc
from mapped_audio_interop import packets, wave, wave_pcm

FIXTURE = Path(__file__).resolve().parent.parent / 'tests/audio/mapped/family1-6ch-gain0.opus'


class MappedFileEvidenceTests(unittest.TestCase):
    def test_complete_native_file_and_crc_sequence_eos_continuation_faults(self):
        original = FIXTURE.read_bytes()
        parsed, granule = packets(original)
        self.assertEqual(len(parsed), 8)
        self.assertEqual(granule, 5760)
        for offset, mask in [(0, 1), (22, 1), (len(original) - 1, 1)]:
            bad = bytearray(original)
            bad[offset] ^= mask
            with self.assertRaises(ValueError):
                packets(bad)
        # Recompute CRC so flag/sequence errors reach their own validation.
        for offset, value in [(5, 3), (5, 0), (18, 1)]:
            bad = bytearray(original)
            bad[offset] = value
            end = 27 + bad[26] + sum(bad[27:27 + bad[26]])
            bad[22:26] = bytes(4)
            bad[22:26] = struct.pack('<I', crc(bad[:end]))
            with self.assertRaises(ValueError):
                packets(bad)
        for bad in [b'', original[:-1], original + b'\0']:
            with self.assertRaises(ValueError):
                packets(bad)

    def test_wave_evidence_rejects_wrong_mask_length_format_and_rate(self):
        original = wave(48000, 6, True, 961)
        self.assertEqual(len(wave_pcm(original, 6, True, 961)), 961 * 6 * 2)
        for offset in [4, 20, 22, 24, 40, 60, 64]:
            bad = bytearray(original)
            bad[offset] ^= 1
            with self.assertRaises(ValueError):
                wave_pcm(bad, 6, True, 961)
        with self.assertRaises(ValueError):
            wave_pcm(original[:-2], 6, True, 961)


if __name__ == '__main__':
    unittest.main()
