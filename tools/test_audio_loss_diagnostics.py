"""Replay plans must preserve the timeline and use only the next packet's FEC."""
from pathlib import Path
import struct
import tempfile
import unittest
import audio_loss_diagnostics as diagnostics


class LossReplayTests(unittest.TestCase):
    def test_burst_and_final_losses_cannot_reuse_future_primary_as_audio(self):
        self.assertEqual(diagnostics.replay_plan(6, {1, 2, 5}),
                         [(0, False), (None, False), (3, True), (3, False), (4, False), (None, False)])
        self.assertEqual(diagnostics.replay_plan(2, {0}), [(1, True), (1, False)])
        self.assertEqual(diagnostics.replay_plan(2, {0, 1}), [(None, False), (None, False)])
        self.assertEqual(diagnostics.replay_plan(2, set()), [(0, False), (1, False)])
        for count, lost in [(0, set()), (1, {-1}), (1, {1}), (1, {0.5})]:
            with self.assertRaises(ValueError):
                diagnostics.replay_plan(count, lost)

    def test_retained_packet_boundaries_and_truncation_are_not_ignored(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/'packets.bin'
            path.write_bytes(struct.pack('<H', 3)+b'abc'+struct.pack('<H', 1)+b'd')
            self.assertEqual(diagnostics.read_reference_packets(path), [b'abc', b'd'])
            for data in [b'', b'\x01', b'\x02\x00x', b'\x00\x00', struct.pack('<H', 1277)+b'x'*1277]:
                path.write_bytes(data)
                with self.assertRaises(ValueError):
                    diagnostics.read_reference_packets(path)


if __name__ == '__main__':
    unittest.main()
