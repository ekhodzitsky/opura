"""Before/after evidence rejects changed PCM, missing samples and stale schedules."""
from copy import deepcopy
import unittest
import decoder_change as change
from test_decoder_bench import sample


def report():
    return dict(schema_version=1, track='decoder_before_after', rounds=15,
                conformance_pcm_hashes_verified=120,
                hardware_before={'cpu': 0, 'cpu_model': 'test CPU', 'machine': 'x86_64', 'kernel': 'test kernel'},
                hardware_after={'cpu': 0, 'cpu_model': 'test CPU', 'machine': 'x86_64', 'kernel': 'test kernel'},
                rustc='rustc 1.87.0',
                source_sha256={'before': '0'*64, 'after': '1'*64},
                binary_sha256={'before': '2'*64, 'after': '3'*64},
                driver_sha256='4'*64, archive_sha256=change.conformance.MANIFEST['archive_sha256'],
                order=change.paired_schedule(15),
                cases={f'{r}/{c}': {'before': [sample(r, 400000000) for _ in range(15)],
                                   'after': [sample(r, 320000000) for _ in range(15)]}
                       for r, c in change.CASES})


class ChangeEvidenceTests(unittest.TestCase):
    def test_complete_pair_has_reproducible_summary(self):
        result = change.summarize(report())
        self.assertEqual(result['aggregate'], {'ratio': .8, 'ci95': [.8, .8]})
        self.assertEqual(len(result['cases']), 10)

    def test_missing_schedule_provenance_and_samples_reject(self):
        for key, value in [('rounds', 14), ('conformance_pcm_hashes_verified', 119),
                           ('order', []), ('source_sha256', {}), ('archive_sha256', '0'*64)]:
            damaged = report(); damaged[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                change.summarize(damaged)
        damaged = report(); damaged['cases']['48000/2']['after'].pop()
        with self.assertRaises(ValueError): change.summarize(damaged)

    def test_missing_or_changed_hardware_and_compiler_reject(self):
        for key, value in [('hardware_before', None), ('hardware_after', {}), ('rustc', '')]:
            damaged = report(); damaged[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                change.summarize(damaged)
        for key, value in [('cpu', 1), ('cpu', False), ('machine', 'changed'),
                           ('cpu_model', ''), ('kernel', 'changed')]:
            damaged = report(); damaged['hardware_after'][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                change.summarize(damaged)

    def test_changed_pcm_latency_counts_and_short_timings_reject(self):
        for key, value in [('fingerprint', 'f'*16), ('elapsed_ns', 1), ('elapsed_ns', True),
                           ('decoder_bytes', 1), ('samples_per_pass', 1), ('mode_packets', [0, 0, 0])]:
            damaged = deepcopy(report()); damaged['cases']['48000/2']['after'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                change.summarize(damaged)


if __name__ == '__main__': unittest.main()
