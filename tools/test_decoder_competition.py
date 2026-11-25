"""Keep pairwise decode timings on the same set of qualified packets."""
import unittest

from benchmarks.competitor.summarize import summary


class CommonStreamTests(unittest.TestCase):
    def setUp(self):
        self.names = ("opura", "opus-rs")
        self.report = {
            "rounds": 1,
            "seed": 8251,
            "cases": [
                {"vector": i, "rate": 48000, "packets": packets,
                 "validation": {"opura": {"passed": True}, "opus-rs": {"passed": ok}}}
                for i, packets, ok in [(1, 2, True), (7, 8, False)]
            ],
            "runs": [
                [1, 48000, 1, "opura", 6000, 4000],
                [1, 48000, 1, "opus-rs", 4000, 2500],
                [1, 48000, 7, "opura", 80000, 11000],
            ],
        }

    def test_excludes_unqualified_stream_from_both_times_and_packet_count(self):
        result = summary(self.report, 48000, self.names)
        self.assertEqual(result["vectors"], [1])
        self.assertEqual(result["packets"], 2)
        self.assertEqual(result["median_us_per_packet"], [3.0, 2.0])
        self.assertEqual(result["opura_to_other_paired_median"], 1.5)

    def test_missing_timing_for_qualified_stream_fails(self):
        self.report["runs"].pop(1)
        with self.assertRaisesRegex(ValueError, "incomplete observation"):
            summary(self.report, 48000, self.names)
