"""Regression tests preventing unsupported or incomplete competitive wins."""
import copy
import hashlib
import struct
import unittest

from competitive_gate import evaluate
from packet_bench import corpus


def evidence():
    cases = {}
    for name, packets in corpus().items():
        data = b''.join(struct.pack('<I', len(p)) + p for p, _ in packets)
        cases[name] = dict(correct=True, packets=len(packets), corpus_sha256=hashlib.sha256(data).hexdigest(),
                           samples_ns={'opura': [8.0] * 15, 'libopus': [10.0] * 15})
    return dict(schema=1, track='packet', rounds=15, cases=cases, revision='a' * 40,
                source_sha256='b' * 64, libopus=dict(version='test', sha256='c' * 64))


class CompetitiveGateTests(unittest.TestCase):
    def test_controlled_packet_win_is_not_decoder_win(self):
        self.assertEqual(evaluate(evidence()), [])
        self.assertTrue(evaluate(evidence(), 'decoder'))

    def test_missing_and_corrupt_evidence(self):
        base = evidence()
        bad = [None, [], dict(), dict(base, cases={}), dict(base, rounds=1),
               dict(base, revision=''), dict(base, source_sha256=''), dict(base, libopus={})]
        for key, value in [('correct', False), ('packets', 1), ('corpus_sha256', 'changed'),
                           ('samples_ns', {'opura': [1] * 15})]:
            report = copy.deepcopy(base)
            report['cases']['single'][key] = value
            bad.append(report)
        for values in [[1] * 14, [0] * 15, [-1] * 15, [float('nan')] * 15, [float('inf')] * 15]:
            report = copy.deepcopy(base)
            report['cases']['single']['samples_ns']['opura'] = values
            bad.append(report)
        for report in bad:
            with self.subTest(report=report):
                self.assertTrue(evaluate(report))

    def test_slow_or_tied_results_cannot_pass(self):
        for timing in [9.1, 10.0, 12.0]:
            report = evidence()
            for case in report['cases'].values():
                case['samples_ns']['opura'] = [timing] * 15
            self.assertTrue(evaluate(report))

    def test_aggregate_cannot_hide_a_regression(self):
        report = evidence()
        report['cases']['invalid']['samples_ns']['opura'] = [10.6] * 15
        self.assertTrue(evaluate(report))


if __name__ == '__main__':
    unittest.main()
