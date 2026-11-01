"""Reject tampered or incomplete full-decoder timing evidence."""
from copy import deepcopy
import unittest
import decoder_bench as bench


def sample(rate=48000, elapsed=200_000_000):
    counts=[7000,5000,8075]
    return {'packets':bench.PACKETS_PER_PASS,'passes':1,'samples_per_pass':bench.SAMPLES48_PER_PASS*rate//48000,
            'fingerprint':'0123456789abcdef','decoder_bytes':34000,'elapsed_ns':elapsed,
            'latency_ns':{'p50':5000,'p95':15000,'p99':20000,'max':30000},
            'mode_packets':counts,'mode_latency_total_ns':[v*10000 for v in counts]}


def report(ratio=.8):
    result={'schema_version':1,'track':'decoder_scalar','correct':True,'packets_per_pass':bench.PACKETS_PER_PASS,
            'cflags':bench.CFLAGS[:],'reference_revision':bench.REVISION,'archive_sha256':bench.MANIFEST['archive_sha256'],
            'rounds':15,'warmup_passes':1,'seed':6716,'minimum_sample_ms':200,'conformance_pcm_hashes_verified':120,
            'hardware_before':{'cpu':0},'hardware_after':{'cpu':0},'order':bench.schedule(15),'cases':{}}
    for key in ['source_sha256','corpus_sha256','rust_binary_sha256','reference_binary_sha256','conformance_report_sha256']:
        result[key]='0'*64
    for rate,channels in bench.CASES:
        result['cases'][f'{rate}/{channels}']={'correct':True,'passes':1,'calibration':{'opura':sample(rate),'libopus':sample(rate)},
                                             'samples':{'opura':[sample(rate,int(200_000_000*ratio)) for _ in range(15)],'libopus':[sample(rate) for _ in range(15)]}}
    return result


class DecoderBenchTests(unittest.TestCase):
    def test_valid_target_and_every_slow_case_stay_visible(self):
        self.assertEqual(bench.evaluate(report()),[])
        slower=report(1.5)
        errors=bench.evaluate(slower)
        self.assertEqual(len(errors),11)
        self.assertTrue(any('aggregate' in e for e in errors))
        one=report()
        for row in one['cases']['8000/1']['samples']['opura']:
            row['elapsed_ns']=400_000_000
        self.assertTrue(any('8000/1' in e for e in bench.evaluate(one)))

    def test_provenance_profile_coverage_and_schedule_fail_closed(self):
        valid=report()
        for key,value in [('track','packet'),('correct',False),('packets_per_pass',1),('reference_revision','0'*40),
                          ('archive_sha256','0'*64),('source_sha256','bad'),('cflags',[]),('rounds',True),('rounds',14),
                          ('rounds',101),('conformance_pcm_hashes_verified',119),('warmup_passes',0),('seed',1),
                          ('hardware_before',{}),('hardware_after',{'cpu':1}),('minimum_sample_ms',0),('order',[])]:
            damaged=deepcopy(valid);damaged[key]=value
            with self.subTest(key=key,value=value):self.assertTrue(bench.evaluate(damaged))
        damaged=deepcopy(valid);damaged['cases'].pop('48000/2');self.assertTrue(bench.evaluate(damaged))
        damaged=deepcopy(valid);damaged['order'][0]=deepcopy(damaged['order'][1]);self.assertTrue(bench.evaluate(damaged))

    def test_corrupted_pcm_counts_state_latency_and_short_samples_are_rejected(self):
        valid=report()
        mutations=[('fingerprint','f'*16),('packets',bench.PACKETS_PER_PASS-1),('passes',True),('samples_per_pass',1),
                   ('decoder_bytes',1),('elapsed_ns',1),('elapsed_ns',float('nan')),('elapsed_ns',True),
                   ('latency_ns',{'p50':0,'p95':2,'p99':3,'max':4}),
                   ('latency_ns',{'p50':10,'p95':5,'p99':30,'max':40}),
                   ('mode_packets',[0,0,0]),('mode_latency_total_ns',[0,0,0])]
        for key,value in mutations:
            damaged=deepcopy(valid);damaged['cases']['48000/2']['samples']['opura'][0][key]=value
            with self.subTest(key=key):self.assertTrue(bench.evaluate(damaged))
        damaged=deepcopy(valid);damaged['cases']['48000/2']['samples']['opura'].pop();self.assertTrue(bench.evaluate(damaged))
        damaged=deepcopy(valid);damaged['cases']['48000/2']['samples']=[];self.assertTrue(bench.evaluate(damaged))
        damaged=deepcopy(valid);damaged['cases']['48000/2']['calibration']=None;self.assertTrue(bench.evaluate(damaged))

    def test_adapter_numbers_require_exact_integer_types_and_bounds(self):
        good=sample();self.assertEqual(bench.check_sample(good,bench.PACKETS_PER_PASS,1),good)
        for packets,passes in [(True,1),(bench.PACKETS_PER_PASS,True),(0,1),(bench.PACKETS_PER_PASS,1001)]:
            with self.assertRaises(ValueError):bench.check_sample(good,packets,passes)
        mismatch=deepcopy(good);mismatch['mode_latency_total_ns']=[0,0,0]
        with self.assertRaises(ValueError):bench.check_sample(mismatch,bench.PACKETS_PER_PASS,1)

    def test_conformance_requires_complete_unique_bounded_geometry(self):
        result={'schema_version':1,'complete':True,'passed':True,'archive_sha256':bench.MANIFEST['archive_sha256'],'results':[]}
        for number in range(1,13):
            for rate,channels in bench.CASES:
                result['results'].append({'vector':number,'rate':rate,'channels':channels,'passed':True,'pcm_sha256':'0'*64,
                                          'packets':1,'range_checks':1,'samples_per_channel':120*rate//48000})
        self.assertEqual(len(bench.conformance_cases(result)),120)
        for key,value in [('vector',[]),('rate',True),('channels',3),('pcm_sha256','bad'),('passed',False),('range_checks',0)]:
            damaged=deepcopy(result);damaged['results'][0][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):bench.conformance_cases(damaged)
        damaged=deepcopy(result);damaged['results'][1]=damaged['results'][0]
        with self.assertRaises(ValueError):bench.conformance_cases(damaged)


if __name__=='__main__':unittest.main()
