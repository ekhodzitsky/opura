from copy import deepcopy
from pathlib import Path
import tempfile
import unittest
import encoder_bench as bench


def sample(case):
    frames = case['frames']
    return dict(packets=frames, samples_per_pass=frames*case['n'], packet_bytes=frames*80,
                fingerprint='1234567890abcdef', encoder_object_bytes=65536,
                lookahead=case['lookahead'], passes=1, elapsed_ns=300_000_000, init_ns=1000,
                latency_ns=dict(p50=10000,p95=20000,p99=30000,max=50000), mode_packets=[frames,0,0])


class EncoderBenchmarkIntegrity(unittest.TestCase):
    def test_counts_types_ranges_and_verified_identity_fail_closed(self):
        case=bench.config();case['frames']=100
        valid=sample(case)
        bench.check_sample(valid,case,1)
        for field,value in [('packets',99),('passes',True),('elapsed_ns',0),('elapsed_ns',float('nan')),
                            ('fingerprint','bad'),('fingerprint',1234567890123456),('lookahead',120),('mode_packets',[1,0,0]),
                            ('latency_ns',dict(p50=30,p95=20,p99=40,max=50)),('packet_bytes',1)]:
            damaged=deepcopy(valid);damaged[field]=value
            with self.subTest(field=field,value=value),self.assertRaises(ValueError):
                bench.check_sample(damaged,case,1)
        damaged=deepcopy(valid);damaged['fingerprint']='f'*16
        with self.assertRaises(ValueError):bench.check_sample(damaged,case,1,valid)

    def test_corpus_geometry_padding_and_explicit_exclusions(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'input.bin';case=bench.prepare(path,bench.config(rate=8000),[1]*161)
            self.assertEqual(case['n'],160)
            self.assertEqual(case['lookahead'],52)
            self.assertEqual(case['frames'],2)
            data=path.read_bytes()
            self.assertEqual(len(data),32+320*2)
            self.assertEqual(data[32+161*2:],bytes((320-161)*2))
            self.assertEqual(bench.HEADER.unpack(data[:32])[-1],2)
            with self.assertRaises(FileExistsError):bench.prepare(path,bench.config(),[0]*960)
        with self.assertRaises(ValueError):bench.config(application=3,frame48=120)
        with self.assertRaises(ValueError):bench.config(channels=3)
        with self.assertRaises(ValueError):bench.config(frame48=100)



def report_fixture(profile='smoke', rounds=3):
    import math
    result=dict(opura_version=bench.VERSION,schema_version=1,track='encoder_scalar',status='pass',source_sha256='1'*64,
                base_revision='2'*40,reference_revision=bench.REVISION,reference_checkout_modified=False,
                seed=6716,cflags=bench.CFLAGS,rustflags=bench.RUST_FLAGS,rust_release_profile=bench.RUST_PROFILE,profile=profile,rounds=rounds,warmup=1,minimum_ms=20,
                baseline_complete=profile=='full' and rounds>=15,rustc='rustc',cc='cc',
                initialization_scope='creation',steady_state_scope='public encode',memory_scope='object and process',
                hardware_before=dict(cpu=0,cpu_model='test',machine='test',affinity=[0],thermal_millidegrees={},thermal_note='unavailable',frequency_locked=False,exclusive_cpu=False),hardware_after=dict(cpu=0,cpu_model='test',machine='test',affinity=[0],thermal_millidegrees={},thermal_note='unavailable',frequency_locked=False,exclusive_cpu=False),
                binary_sha256=dict(opura='3'*64,libopus='4'*64),
                fault_sensitivity={k:'rejected_by_both' for k in ['wrong_range','wrong_pcm','oversized_packet','wrong_geometry']},
                cases={},order=[])
    for name,settings,source in bench.specifications(profile):
        c=dict(settings)
        original=bench.input_frames(name,settings,source)
        frames=(original+c['lookahead']+c['n']-1)//c['n']
        c.update(original_frames=original,frames=frames,source_recording_sha256=bench.CORPUS[source] if source else None,
                 corpus_sha256='5'*64,cross_decoder_verified=True,preflight={},samples={},calibration={},
                 validation_record_sha256=dict(opura='6'*64,libopus='7'*64),packet_diagnostics={})
        for impl in ['opura','libopus']:
            v=sample(c);v['elapsed_ns']=300_000_000 if impl=='opura' else 150_000_000
            v['process_maxrss_kib']=4096
            if impl=='libopus':v['native_in_dtx_frames']=int(c['dtx']>0)
            if settings['application'] in [2,4]:v['mode_packets']=[0,0,frames]
            c['preflight'][impl]=deepcopy(v);c['samples'][impl]=[deepcopy(v) for _ in range(rounds)]
            c['calibration'][impl]=min(1000,max(1,math.ceil(20*1250000/v['elapsed_ns'])))
            c['packet_diagnostics'][impl]=dict(verified_packets=frames,bandwidth_packets=[0,0,0,0,frames],
                                             wire_channel_packets=[frames,0],lbrr_packets=int(c['fec']>0),empty_frames=0,packets_with_empty_frame=0,one_byte_frames=int(c['dtx']>0))
        c['summary']=dict(opura_to_libopus_ratio=2,ratio95_lower=2,ratio95_upper=2)
        result['cases'][name]=c
        for i in range(rounds):result['order'].append(dict(round=i,case=name,implementations=['opura','libopus']))
    result['order']=bench.schedule(list(result['cases']),rounds)
    result['equal_case_geometric_mean_ratio']=math.exp(math.log(2))
    return result


class EncoderReportProtocol(unittest.TestCase):
    def test_report_rejects_dropped_cases_changed_controls_or_unpaired_results(self):
        valid=report_fixture()
        bench.validate_report(valid)
        with self.assertRaises(ValueError):bench.validate_report(valid,require_full=True)
        for field,value in [('schema_version',True),('profile','unknown'),('rounds',True),('status','failed'),('source_sha256','bad'),
                            ('reference_revision','0'*40),('fault_sensitivity',{}),('order',[]),
                            ('equal_case_geometric_mean_ratio',0.1),('hardware_after',dict(cpu=1,cpu_model='test',machine='test'))]:
            damaged=deepcopy(valid);damaged[field]=value
            with self.subTest(field=field),self.assertRaises(ValueError):bench.validate_report(damaged)
        for field,value in [('cap',1),('cross_decoder_verified',False),('frames',1),('corpus_sha256','bad'),
                            ('samples',dict(opura=[],libopus=[])),('calibration',dict(opura=1000,libopus=1)),
                            ('packet_diagnostics',dict(opura={},libopus={}))]:
            damaged=deepcopy(valid);damaged['cases']['mixed-audio-cbr'][field]=value
            with self.subTest(field=field),self.assertRaises(ValueError):bench.validate_report(damaged)
        damaged=deepcopy(valid);damaged['order'][0]=deepcopy(damaged['order'][1])
        with self.assertRaises(ValueError):bench.validate_report(damaged)
        damaged=deepcopy(valid);damaged['cases'].pop('mixed-silk-cbr')
        with self.assertRaises(ValueError):bench.validate_report(damaged)
        damaged=deepcopy(valid);damaged['cases']['mixed-celt-cbr']['samples']['opura'][0]['elapsed_ns']=1
        with self.assertRaises(ValueError):bench.validate_report(damaged)

    def test_exported_record_binds_metadata_to_bytes_and_cbr_budget(self):
        import struct
        c=bench.config();c.update(frames=1)
        packet=bytes([128])+bytes(79)
        header=struct.pack('<8sIHHII',b'OPURAE01',c['rate'],c['channels'],c['n'],1,c['lookahead'])
        body=struct.pack('<HIQ',80,123,456)+packet
        digest=14695981039346656037
        for value in body:digest=((digest^value)*1099511628211)&0xffffffffffffffff
        v=sample(c);v.update(packet_bytes=80,fingerprint=f'{digest:016x}',mode_packets=[0,0,1])
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'record';path.write_bytes(header+body)
            bench.validation_record(path,c,v)
            for data in [header+body[:-1],header+body+b'x',header+struct.pack('<HIQ',65535,123,456)+packet,
                         bytes([0])+header[1:]+body,header+body[:14]+bytes(80)]:
                path.write_bytes(data)
                with self.assertRaises(ValueError):bench.validation_record(path,c,v)

    def test_control_types_do_not_accept_bool_or_nonintegral_values(self):
        for field,value in [('complexity',True),('rate',48000.0),('cap',False),('loss',101),('mode',3),('application',5)]:
            with self.subTest(field=field),self.assertRaises(ValueError):bench.config(**{field:value})


class CompleteEncoderReport(unittest.TestCase):
    def test_full_evidence_and_derived_confidence_intervals_cannot_be_forged(self):
        valid=report_fixture('full',15)
        bench.summarize(valid)
        bench.validate_report(valid,require_full=True)
        self.assertFalse(valid['scalar_speed_only_target_met'])
        empty=deepcopy(valid)
        for d in empty['cases']['voip-dtx1-loss0']['packet_diagnostics'].values():
            d.update(one_byte_frames=0,empty_frames=5,packets_with_empty_frame=5)
        bench.validate_report(empty)  # Generalized DTX uses empty compressed frames.

        for name,key in [('voip-dtx1-loss0','one_byte_frames'),('voip-dtx0-loss20','lbrr_packets')]:
            damaged=deepcopy(valid);damaged['cases'][name]['packet_diagnostics']['opura'][key]=0
            with self.subTest(name=name),self.assertRaises(ValueError):bench.validate_report(damaged)

        # JSON reports sort keys; confidence draws must not depend on insertion order.
        varied=report_fixture('full',15)
        for case_index,case in enumerate(varied['cases'].values()):
            for i,row in enumerate(case['samples']['opura']):row['elapsed_ns']=220_000_000+((i*719+case_index*113)%29)*10_000_000
            for i,row in enumerate(case['samples']['libopus']):row['elapsed_ns']=150_000_000+((i*317+case_index*137)%23)*5_000_000
        bench.summarize(varied)
        import json
        bench.validate_report(json.loads(json.dumps(varied,sort_keys=True)),require_full=True)
        damaged=deepcopy(valid);case=damaged['cases']['voip-dtx1-loss0']
        for row in [case['preflight']['libopus'],*case['samples']['libopus']]:row['native_in_dtx_frames']=0
        with self.assertRaises(ValueError):bench.validate_report(damaged)
        damaged=deepcopy(valid);damaged['cases']['mixed-audio-cbr']['summary']['ratio95_upper']=0.01
        with self.assertRaises(ValueError):bench.validate_report(damaged,require_full=True)
        damaged=deepcopy(valid);damaged['aggregate_ratio95_upper']=0.01
        with self.assertRaises(ValueError):bench.validate_report(damaged,require_full=True)
        damaged=deepcopy(valid);damaged['scalar_speed_only_target_met']=True
        with self.assertRaises(ValueError):bench.validate_report(damaged,require_full=True)


class ConservativeConfidenceBounds(unittest.TestCase):
    def test_interval_preserves_old_upper_bound_and_round_pairing(self):
        import random
        from competitive_gate import paired_interval, upper_bound
        a=[10,21,7,40,31,11,13,25,18,3,17,13,99,1,44]
        b=[8,15,4,31,28,8,9,20,15,2,14,10,80,1,31]
        low,high=paired_interval(a,b,random.Random(6716))
        self.assertEqual(high,upper_bound(a,b,random.Random(6716)))
        self.assertTrue(0<low<=high)
        self.assertEqual(paired_interval([2]*15,[1]*15,random.Random(1)),(2,2))


if __name__=='__main__':unittest.main()
