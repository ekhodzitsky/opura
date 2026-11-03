#!/usr/bin/env python3
"""Optional paired in-process public encoder baseline; no perceptual/SOTA claim."""
import argparse
from copy import deepcopy
from array import array
from datetime import datetime, timezone
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import random
import re
import statistics
import struct
import subprocess
import sys
import tomllib

from audio_baseline import CORPUS, REVISION, read_wave, sha
from check_conformance import ROOT, write_report
from competitive_gate import paired_interval
from decoder_bench import hardware, CFLAGS as BASE_CFLAGS

CFLAGS = [*BASE_CFLAGS, *(['-march=x86-64', '-mtune=generic'] if platform.machine()=='x86_64' else [])]
RUST_PROFILE = {'OPT_LEVEL':'3','CODEGEN_UNITS':'16','LTO':'false','PANIC':'unwind','DEBUG':'false','DEBUG_ASSERTIONS':'false','OVERFLOW_CHECKS':'false','STRIP':'none','INCREMENTAL':'false'}
RUST_FLAGS = '-C target-cpu=x86-64' if platform.machine()=='x86_64' else '-C target-cpu=generic'

HEADER = struct.Struct('<8sIHHIHBBBBBBI')
APPS = ('audio', 'voip', 'lowdelay', 'silk', 'celt')
MODES = ('cbr', 'vbr', 'cvbr')
VERSION = tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
SAMPLE_KEYS = ('packets', 'samples_per_pass', 'packet_bytes', 'fingerprint',
               'encoder_object_bytes', 'lookahead', 'mode_packets')


def fingerprint_sources():
    files = [*ROOT.glob('src/**/*.rs'), *[ROOT/name for name in [
        'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'examples/encoder_bench.rs',
        'tools/encoder_bench.py', 'tools/benchmarks/encoder.c', 'tools/test_encoder_bench.py',
        'tools/audio_baseline.py', 'tools/decoder_bench.py', 'tools/competitive_gate.py',
        'tools/check_conformance.py', 'tools/conformance_manifest.json', 'tools/packet_bench.py']]]
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(str(path.relative_to(ROOT)).encode()+b'\0'+path.read_bytes()+b'\0')
    return digest.hexdigest()


def config(rate=48000, channels=1, frame48=960, application=0, complexity=10,
           mode=0, bitrate=32000, cap=None, loss=0, fec=0, dtx=0):
    if any(type(x) is not int for x in [rate,channels,frame48,application,complexity,mode,bitrate,loss,fec,dtx]) or (cap is not None and type(cap) is not int):
        raise ValueError('Controls require exact integer types')
    if rate not in [8000, 12000, 16000, 24000, 48000] or channels not in [1, 2]:
        raise ValueError('Invalid rate/channels')
    if frame48 not in [120, 240, 480, 960, 1920, 2880, 3840, 4800, 5760]:
        raise ValueError('Invalid frame geometry')
    if not (0 <= application < 5 and 0 <= complexity <= 10 and 0 <= mode <= 2
            and 1 <= bitrate <= 1500000 and 0 <= loss <= 100 and fec in [0, 1] and dtx in [0, 1]):
        raise ValueError('Invalid controls')
    if application == 3 and frame48 < 480:
        raise ValueError('Restricted SILK requires at least10ms')
    if cap is None:
        cap = 7656 if frame48 > 960 else 1276
    if not 1 <= cap <= (7656 if frame48 > 960 else 1276):
        raise ValueError('Invalid cap')
    return dict(rate=rate, channels=channels, n=frame48*rate//48000,
                application=application, complexity=complexity, mode=mode, bitrate=bitrate,
                cap=cap, loss=loss, fec=fec, dtx=dtx,
                lookahead=(120 if application in [2, 4] else 312)*rate//48000)


def prepare(path, settings, pcm):
    c = dict(settings)
    if len(pcm) % c['channels'] or not pcm:
        raise ValueError('Invalid PCM layout')
    original = len(pcm)//c['channels']
    c['frames'] = (original+c['lookahead']+c['n']-1)//c['n']
    samples = c['frames']*c['n']*c['channels']
    if 32+samples*2 > 64*1024*1024 or not 1 <= c['frames'] <= 100000:
        raise ValueError('Corpus exceeds adapter bounds')
    data = array('h', pcm)
    data.extend([0]*(samples-len(data)))
    if sys.byteorder != 'little':
        data.byteswap()
    header = HEADER.pack(b'OPURAB01', c['rate'], c['channels'], c['n'], c['bitrate'],
                         c['cap'], c['application'], c['complexity'], c['mode'],
                         c['loss'], c['fec'], c['dtx'], c['frames'])
    with path.open('xb') as output:
        output.write(header)
        output.write(data.tobytes())
    c.update(original_frames=original, corpus_sha256=sha(path))
    return c


def check_sample(sample, case, passes, expected=None):
    if not isinstance(sample, dict) or type(passes) is not int or not 1 <= passes <= 1000:
        raise ValueError('Invalid sample/pass count')
    required = ['packets', 'samples_per_pass', 'packet_bytes', 'encoder_object_bytes',
                'lookahead', 'passes', 'elapsed_ns', 'init_ns']
    if any(type(sample.get(k)) is not int or sample[k] <= 0 for k in required):
        raise ValueError('Invalid positive adapter field')
    if (sample['passes'] != passes or sample['packets'] != case['frames']
            or sample['samples_per_pass'] != case['frames']*case['n']
            or sample['lookahead'] != case['lookahead']
            or not case['frames'] <= sample['packet_bytes'] <= case['frames']*case['cap']
            or sample['encoder_object_bytes'] > 16*1024*1024
            or sample['elapsed_ns'] > 10**15 or sample['init_ns'] > 10**12):
        raise ValueError('Adapter geometry/control mismatch')
    if not isinstance(sample.get('fingerprint'),str) or not re.fullmatch('[0-9a-f]{16}', sample['fingerprint']):
        raise ValueError('Missing packet/range/PCM fingerprint')
    latency = sample.get('latency_ns')
    if (not isinstance(latency, dict) or set(latency) != {'p50', 'p95', 'p99', 'max'}
            or any(type(v) is not int or not 0 < v <= 10**12 for v in latency.values())
            or not latency['p50'] <= latency['p95'] <= latency['p99'] <= latency['max']):
        raise ValueError('Invalid latency distribution')
    counts = sample.get('mode_packets')
    if (not isinstance(counts, list) or len(counts) != 3
            or any(type(n) is not int or n < 0 for n in counts) or sum(counts) != case['frames']):
        raise ValueError('Invalid mode counts')
    if (case['application'] in [2,4] and counts!=[0,0,case['frames']]) or (case['application']==3 and counts!=[case['frames'],0,0]):
        raise ValueError('Restricted application emitted a different mode')
    if sample['elapsed_ns']/passes < sample['latency_ns']['p50']*case['frames']/20:
        raise ValueError('Implausibly short timed pass')
    if expected and any(sample[k] != expected[k] for k in SAMPLE_KEYS):
        raise ValueError('Measured output changed from cross-verified preflight')
    return sample


def compile_adapters(source, folder):
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Pinned reference required')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    def manifest(file, key):
        match = re.search(r'^'+key+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if not match:
            raise ValueError('Missing native manifest')
        return match[1].replace('\\', '').split()
    files = sum([manifest(*v) for v in [('silk_sources.mk', 'SILK_SOURCES'),
                ('silk_sources.mk', 'SILK_SOURCES_FIXED'), ('celt_sources.mk', 'CELT_SOURCES'),
                ('opus_sources.mk', 'OPUS_SOURCES')]], [])
    native = folder/'libopus-encoder'
    subprocess.run(['cc', *CFLAGS, *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                    str(ROOT/'tools/benchmarks/encoder.c'), *[str(source/f) for f in files],
                    '-lm', '-o', str(native)], check=True)
    env = dict(os.environ)
    for key in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET']:
        env.pop(key, None)
    env['RUSTFLAGS']=RUST_FLAGS
    env.update({'CARGO_PROFILE_RELEASE_'+k:v for k,v in RUST_PROFILE.items()})
    rustc = subprocess.check_output(['rustc', '-Vv'], cwd=ROOT, text=True)
    target = next(line.split(': ', 1)[1] for line in rustc.splitlines() if line.startswith('host: '))
    subprocess.run(['cargo', 'build', '--offline', '--locked', '--release', '--example', 'encoder_bench',
                    '--target-dir', str(folder/'rust'), '--target', target], cwd=ROOT, env=env, check=True)
    return {'opura': folder/'rust'/target/'release/examples/encoder_bench', 'libopus': native}, rustc


def synthetic(rate, channels, seconds=2):
    values = array('h')
    rng = 6716
    for frame in range(rate*seconds):
        for channel in range(channels):
            rng ^= rng << 13 & 0xffffffff; rng ^= rng >> 17; rng ^= rng << 5 & 0xffffffff
            phase = frame % (rate//200)
            part = (frame*5//rate) % 5
            x = (rng & 65535)-32768
            if part == 0:
                x = (12000 if phase < rate//400 else -12000)+(x >> 4)
            elif part == 1:
                x >>= 2
            elif part == 2:
                x = 28000 if frame % (rate//100) == 0 else 0
            elif part == 3:
                x = 0
            else:
                x = x//2 if channel else x//4
            values.append(x)
    return values


RECORDINGS = {'speech_orig': (1, 518400), 'music_orig': (2, 4358219)}


def specifications(profile):
    if profile not in ['smoke', 'full']:
        raise ValueError('Unknown workload profile')
    result = []
    for app in range(5):
        for mode in range(3):
            result.append((f'mixed-{APPS[app]}-{MODES[mode]}', config(application=app, mode=mode), None))
    if profile == 'full':
        for rate in [8000, 12000, 16000, 24000, 48000]:
            for channels in [1, 2]:
                result.append((f'mixed-{rate}-{channels}', config(rate=rate, channels=channels, mode=1, bitrate=64000), None))
        for frame in [120, 240, 480, 1920, 2880, 3840, 4800, 5760]:
            result.append((f'frame-{frame}', config(channels=2, frame48=frame, mode=2, bitrate=64000), None))
        for dtx, loss in [(1, 0), (0, 20), (1, 20)]:
            result.append((f'voip-dtx{dtx}-loss{loss}', config(application=1, mode=1, dtx=dtx, loss=loss, fec=int(loss>0)), None))
        for name, (channels, _) in RECORDINGS.items():
            for app in [0, 1, 2, 3]:
                for mode in range(3):
                    result.append((f'{name}-{APPS[app]}-{MODES[mode]}', config(channels=channels, application=app, mode=mode, bitrate=32000 if channels==1 else 64000), name))
    return result


def cases(profile, corpus):
    recordings = {}
    result = []
    for name, settings, source in specifications(profile):
        if source:
            if source not in recordings:
                path = corpus/(source+'.wav')
                if sha(path) != CORPUS[source]:
                    raise ValueError('Recording checksum mismatch')
                channels, pcm = read_wave(path)
                if (channels, len(pcm)//channels) != RECORDINGS[source]:
                    raise ValueError('Recording geometry mismatch')
                recordings[source] = pcm
            result.append((name, settings, recordings[source], CORPUS[source]))
        else:
            result.append((name, settings, synthetic(settings['rate'],settings['channels']), None))
    return result


def invoke(binary, path, case, passes, warmup, record, rss):
    command = ['/usr/bin/time', '-f', '%M', '-o', str(rss), str(binary), str(path),
               str(record) if record else '-', str(passes), str(warmup)]
    result = subprocess.run(command, capture_output=True, text=True, timeout=600)
    if result.returncode:
        raise ValueError('Adapter failed: '+result.stderr[-2000:])
    sample = check_sample(json.loads(result.stdout), case, passes)
    memory = int(rss.read_text().strip())
    if memory <= 0:
        raise ValueError('Missing process RSS')
    sample['process_maxrss_kib'] = memory
    return sample


def verify(binary, corpus, record, frames):
    result = subprocess.run([str(binary), 'verify', str(corpus), str(record)], capture_output=True, text=True, timeout=180)
    if result.returncode or json.loads(result.stdout).get('verified_packets') != frames:
        raise ValueError('Independent cross-decoder verification failed: '+result.stderr[-1000:])
    return json.loads(result.stdout)


def fault_probes(binaries, corpus, record, output):
    original = record.read_bytes()
    if len(original) < 39:
        raise ValueError('Missing validation record')
    mutations = {'wrong_range': (26, original[26]^1), 'wrong_pcm': (30, original[30]^1),
                 'oversized_packet': (24, 255), 'wrong_geometry': (8, original[8]^1)}
    results = {}
    for name, (offset, value) in mutations.items():
        data = bytearray(original)
        data[offset] = value
        if name == 'oversized_packet':
            data[25] = 255
        path = output/(name+'.bin')
        path.write_bytes(data)
        for implementation, binary in binaries.items():
            result = subprocess.run([str(binary), 'verify', str(corpus), str(path)], capture_output=True, timeout=180)
            if not result.returncode:
                raise ValueError('Corrupted validation accepted: '+implementation+'/'+name)
        results[name] = 'rejected_by_both'
    return results


def schedule(names, rounds):
    rng=random.Random(6716)
    rows=[]
    for round_number in range(rounds):
        order=list(names);rng.shuffle(order)
        for name in order:
            implementations=['opura','libopus'];rng.shuffle(implementations)
            rows.append(dict(round=round_number,case=name,implementations=implementations))
    return rows


def summarize(report):
    rng = random.Random(6716)
    ratios = []
    for name in sorted(report['cases']):
        case=report['cases'][name]
        samples = case['samples']
        a = [s['elapsed_ns']/s['passes'] for s in samples['opura']]
        b = [s['elapsed_ns']/s['passes'] for s in samples['libopus']]
        lower,upper=paired_interval(a,b,rng)
        case['summary'] = dict(opura_to_libopus_ratio=statistics.median(a)/statistics.median(b),
                               ratio95_lower=lower,ratio95_upper=upper,
                               median_ns_per_packet={k: statistics.median(s['elapsed_ns']/s['passes']/s['packets'] for s in v) for k, v in samples.items()},
                               median_latency_ns={k: {q: statistics.median(s['latency_ns'][q] for s in v) for q in ['p50','p95','p99','max']} for k, v in samples.items()})
        case['summary'].update(
            median_init_ns={k:statistics.median(s['init_ns'] for s in v) for k,v in samples.items()},
            median_process_maxrss_kib={k:statistics.median(s['process_maxrss_kib'] for s in v) for k,v in samples.items()},
            effective_packet_bps={k:case['preflight'][k]['packet_bytes']*8/(case['frames']*case['n']/case['rate']) for k in samples},
            mode_mix_equal=case['preflight']['opura']['mode_packets']==case['preflight']['libopus']['mode_packets'],
            validation_records_equal=case['validation_record_sha256']['opura']==case['validation_record_sha256']['libopus'])
        ratios.append(case['summary']['opura_to_libopus_ratio'])
    report['equal_case_geometric_mean_ratio'] = math.exp(statistics.mean(map(math.log, ratios)))
    aggregate = []
    for _ in range(2000):
        indices = rng.choices(range(report['rounds']), k=report['rounds'])
        values = []
        for name in sorted(report['cases']):
            case=report['cases'][name]
            a = case['samples']['opura']; b = case['samples']['libopus']
            values.append(statistics.median(a[i]['elapsed_ns']/a[i]['passes'] for i in indices)/statistics.median(b[i]['elapsed_ns']/b[i]['passes'] for i in indices))
        aggregate.append(math.exp(statistics.mean(map(math.log,values))))
    ordered=sorted(aggregate)
    report['aggregate_ratio95_lower']=ordered[49]
    report['aggregate_ratio95_upper']=ordered[1949]
    report['aggregate_weighting'] = 'Equal weight per declared case; resample the same round indices across all cases.'
    report['scalar_speed_only_target_met'] = report['aggregate_ratio95_upper']<=0.90 and all(c['summary']['ratio95_upper']<=1.05 for c in report['cases'].values())


def validation_record(path, case, sample):
    maximum = min(256*1024*1024, 24+case['frames']*(case['cap']+14))
    with path.open('rb') as stream:
        data = stream.read(maximum+1)
    expected = struct.pack('<8sIHHII', b'OPURAE01', case['rate'], case['channels'],
                           case['n'], case['frames'], case['lookahead'])
    if len(data)>maximum or data[:24]!=expected:
        raise ValueError('Validation record header/bound mismatch')
    offset=24; count=0; total=0; digest=14695981039346656037; modes=[0,0,0]
    while offset<len(data):
        if len(data)-offset<14:
            raise ValueError('Truncated validation header')
        length=struct.unpack_from('<H',data,offset)[0]
        end=offset+14+length
        if not 1<=length<=case['cap'] or end>len(data):
            raise ValueError('Invalid validation payload length')
        if case['mode']==0 and not case['dtx'] and case['bitrate']*case['n']%(case['rate']*8)==0:
            target=min(case['cap'],case['bitrate']*case['n']//(case['rate']*8))
            if length!=target:
                raise ValueError('Whole CBR byte budget mismatch')
        toc=data[offset+14]
        modes[2 if toc&128 else 1 if toc&96==96 else 0]+=1
        for value in data[offset:end]:
            digest=((digest^value)*1099511628211)&0xffffffffffffffff
        count+=1;total+=length;offset=end
    if (count!=case['frames'] or total!=sample['packet_bytes'] or modes!=sample['mode_packets']
            or f'{digest:016x}'!=sample['fingerprint']):
        raise ValueError('Exported packets differ from adapter correctness metadata')


def validate_report(report, require_full=False):
    if not isinstance(report,dict) or type(report.get('schema_version')) is not int or report['schema_version']!=1 or report.get('track')!='encoder_scalar' or report.get('status')!='pass':
        raise ValueError('Complete successful encoder report required')
    if report.get('opura_version')!=VERSION:
        raise ValueError('Wrong measured package version')
    for key,length in [('source_sha256',64),('base_revision',40)]:
        if not isinstance(report.get(key),str) or not re.fullmatch('[0-9a-f]{'+str(length)+'}',report[key]):
            raise ValueError('Missing provenance: '+key)
    if report.get('rustflags')!=RUST_FLAGS or report.get('rust_release_profile')!=RUST_PROFILE:
        raise ValueError('Unmatched Rust target/profile flags')
    if (report.get('reference_revision')!=REVISION or report.get('reference_checkout_modified') is not False
            or report.get('cflags')!=CFLAGS or type(report.get('seed')) is not int or report['seed']!=6716 or type(report.get('rounds')) is not int
            or not 3<=report['rounds']<=100 or type(report.get('warmup')) is not int
            or not 1<=report['warmup']<=100 or type(report.get('minimum_ms')) is not int
            or not 20<=report['minimum_ms']<=2000):
        raise ValueError('Invalid reference/measurement protocol')
    profile=report.get('profile'); specs=specifications(profile)
    complete=profile=='full' and report['rounds']>=15
    if report.get('baseline_complete') is not complete or (require_full and not complete):
        raise ValueError('Full profile and at least15 paired rounds required for the retained baseline')
    for key in ['rustc','cc','initialization_scope','steady_state_scope','memory_scope']:
        if not isinstance(report.get(key),str) or not report[key]:
            raise ValueError('Missing protocol description: '+key)
    for phase in ['hardware_before','hardware_after']:
        h=report.get(phase)
        if not isinstance(h,dict) or type(h.get('cpu')) is not int or h['cpu']<0 or not h.get('cpu_model') or not h.get('machine') or h.get('affinity')!=[h['cpu']] or not isinstance(h.get('thermal_millidegrees'),dict) or not h.get('thermal_note') or h.get('frequency_locked') is not False or h.get('exclusive_cpu') is not False:
            raise ValueError('Missing hardware/affinity evidence')
    if report['hardware_before']['cpu']!=report['hardware_after']['cpu']:
        raise ValueError('Affinity changed across measurements')
    if set(report.get('binary_sha256',{}))!={'opura','libopus'} or any(not isinstance(h,str) or not re.fullmatch('[0-9a-f]{64}',h) for h in report['binary_sha256'].values()):
        raise ValueError('Missing adapter hashes')
    if report.get('fault_sensitivity')!={k:'rejected_by_both' for k in ['wrong_range','wrong_pcm','oversized_packet','wrong_geometry']}:
        raise ValueError('Missing live rejection probes')
    cases=report.get('cases')
    if not isinstance(cases,dict) or set(cases)!={name for name,_,_ in specs}:
        raise ValueError('Missing or extra workload cases')
    rounds=report['rounds']
    for name,settings,source in specs:
        c=cases[name]
        if not isinstance(c,dict) or any(type(c.get(k)) is not type(v) or c[k]!=v for k,v in settings.items()):
            raise ValueError('Declared settings mismatch: '+name)
        original=RECORDINGS[source][1] if source else settings['rate']*2
        frames=(original+settings['lookahead']+settings['n']-1)//settings['n']
        if (c.get('original_frames')!=original or c.get('frames')!=frames or c.get('cross_decoder_verified') is not True
                or c.get('source_recording_sha256')!=(CORPUS[source] if source else None)
                or not isinstance(c.get('corpus_sha256'),str) or not re.fullmatch('[0-9a-f]{64}',c['corpus_sha256'])):
            raise ValueError('Missing corpus or independent decoding proof: '+name)
        for key in ['preflight','samples','calibration','validation_record_sha256','packet_diagnostics']:
            if not isinstance(c.get(key),dict) or set(c[key])!={'opura','libopus'}:
                raise ValueError('Missing paired case field: '+name+'/'+key)
        for impl in ['opura','libopus']:
            if not isinstance(c['validation_record_sha256'][impl],str) or not re.fullmatch('[0-9a-f]{64}',c['validation_record_sha256'][impl]):
                raise ValueError('Missing validation record hash')
            expected=check_sample(c['preflight'][impl],c,1)
            passes=c['calibration'][impl]
            if type(passes) is not int or passes!=min(1000,max(1,math.ceil(report['minimum_ms']*1250000/expected['elapsed_ns']))):
                raise ValueError('Invalid calibration')
            samples=c['samples'][impl]
            if not isinstance(samples,list) or len(samples)!=rounds:
                raise ValueError('Missing measured rounds')
            for sample in [expected,*samples]:
                check_sample(sample,c,1 if sample is expected else passes,expected)
                if type(sample.get('process_maxrss_kib')) is not int or not 0<sample['process_maxrss_kib']<=2*1024*1024:
                    raise ValueError('Missing process RSS scope')
                if sample is not expected and sample['elapsed_ns']<report['minimum_ms']*750000:
                    raise ValueError('Measured sample below calibrated minimum')
            d=c['packet_diagnostics'][impl]
            if not isinstance(d,dict) or d.get('verified_packets')!=frames:
                raise ValueError('Missing native packet diagnostics')
            for key,length in [('bandwidth_packets',5),('wire_channel_packets',2)]:
                values=d.get(key)
                if not isinstance(values,list) or len(values)!=length or any(type(x) is not int or x<0 for x in values) or sum(values)!=frames:
                    raise ValueError('Invalid packet diagnostic counts')
            for key in ['lbrr_packets','packets_with_empty_frame','empty_frames','one_byte_frames']:
                value=d.get(key)
                if type(value) is not int or not 0<=value<=frames*(48 if key in ['empty_frames','one_byte_frames'] else 1):
                    raise ValueError('Invalid recovery/empty diagnostic counts')
            if d['empty_frames']<d['packets_with_empty_frame']:
                raise ValueError('Empty-frame counts disagree')
    order=report.get('order');seen=set()
    if not isinstance(order,list) or len(order)!=rounds*len(cases):
        raise ValueError('Missing paired randomized schedule')
    for row in order:
        if not isinstance(row,dict) or type(row.get('round')) is not int or not 0<=row['round']<rounds or row.get('case') not in cases or row.get('implementations') not in [['opura','libopus'],['libopus','opura']]:
            raise ValueError('Invalid schedule row')
        key=(row['round'],row['case'])
        if key in seen:
            raise ValueError('Duplicate schedule row')
        seen.add(key)
    if order!=schedule([name for name,_,_ in specs],rounds):
        raise ValueError('Schedule differs from declared randomized seed')
    geometric=[]
    for name in sorted(cases):
        case=cases[name]
        a=case['samples']['opura'];b=case['samples']['libopus']
        ratio=statistics.median(s['elapsed_ns']/s['passes'] for s in a)/statistics.median(s['elapsed_ns']/s['passes'] for s in b)
        summary=case.get('summary')
        if not isinstance(summary,dict) or summary.get('opura_to_libopus_ratio')!=ratio or type(summary.get('ratio95_upper')) not in [float,int] or not math.isfinite(summary['ratio95_upper']) or summary['ratio95_upper']<=0:
            raise ValueError('Invalid derived case summary')
        if type(summary.get('ratio95_lower')) not in [float,int] or not math.isfinite(summary['ratio95_lower']) or not 0<summary['ratio95_lower']<=summary['ratio95_upper']:
            raise ValueError('Invalid lower confidence bound')
        geometric.append(ratio)
    if report.get('equal_case_geometric_mean_ratio')!=math.exp(statistics.mean(map(math.log,geometric))):
        raise ValueError('Invalid aggregate weighting/result')
    if require_full:
        recalculated=deepcopy(report);summarize(recalculated)
        if any(report.get(k)!=recalculated[k] for k in ['aggregate_ratio95_lower','aggregate_ratio95_upper','aggregate_weighting','scalar_speed_only_target_met']) or any(cases[k]['summary']!=recalculated['cases'][k]['summary'] for k in cases):
            raise ValueError('Derived confidence intervals/statistics differ from raw paired samples')
    return report


def snapshot(cpu):
    result=hardware(cpu)
    thermal={}
    for zone in sorted(Path('/sys/class/thermal').glob('thermal_zone*')):
        try:
            value=int((zone/'temp').read_text().strip())
            label=(zone/'type').read_text().strip()
            thermal[zone.name+':'+label]=value
        except (OSError,ValueError):
            continue
    result.update(affinity=sorted(os.sched_getaffinity(0)),thermal_millidegrees=thermal,
                  thermal_note='Available sysfs sensors only; empty means unavailable, no temperature control.',
                  load_average=list(os.getloadavg()))
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('source', type=Path, nargs='?'); p.add_argument('corpus', type=Path, nargs='?'); p.add_argument('output', type=Path, nargs='?')
    p.add_argument('--check',type=Path)
    p.add_argument('--preflight-only',action='store_true')
    p.add_argument('--profile', choices=['smoke','full'], default='full')
    p.add_argument('--rounds', type=int, default=15); p.add_argument('--warmup', type=int, default=1)
    p.add_argument('--minimum-ms', type=int, default=200); p.add_argument('--cpu', type=int)
    args = p.parse_args()
    if args.check:
        checked=json.loads(args.check.read_text());validate_report(checked,require_full=True)
        if checked['source_sha256']!=fingerprint_sources():raise ValueError('Stale encoder benchmark source')
        print('Complete baseline report verifies; no perceptual/whole-market superiority claim.');return
    if not args.source or not args.corpus or not args.output:p.error('source, corpus and output are required')
    if not 3 <= args.rounds <= 100 or not 1 <= args.warmup <= 100 or not 20 <= args.minimum_ms <= 2000:
        p.error('Invalid rounds/warmup/calibration bounds')
    if not hasattr(os, 'sched_getaffinity') or not Path('/usr/bin/time').exists():
        p.error('This benchmark adapter requires Linux affinity and GNU time')
    if args.profile=='full' and args.rounds>=15 and not args.preflight_only and subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip():
        raise ValueError('Retained full baseline requires a clean checkout before setup')
    args.output.mkdir(parents=True, exist_ok=False)
    digest = fingerprint_sources()
    build = ROOT/'target/encoder-bench'/digest[:16]; build.mkdir(parents=True, exist_ok=True)
    binaries, rustc = compile_adapters(args.source.resolve(), build)
    report = dict(schema_version=1, track='encoder_scalar', status='in_progress',
                  opura_version=VERSION,
                  source_sha256=digest, base_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
                  reference_revision=REVISION, reference_checkout_modified=False,
                  profile=args.profile, rounds=args.rounds, seed=6716, warmup=args.warmup, minimum_ms=args.minimum_ms,
                  cflags=CFLAGS, rustflags=RUST_FLAGS, rust_release_profile=RUST_PROFILE, rustc=rustc.strip(), cc=subprocess.check_output(['cc','--version'],text=True).splitlines()[0],
                  binary_sha256={k:sha(v) for k,v in binaries.items()}, created_utc=datetime.now(timezone.utc).isoformat(),
                  initialization_scope='Public encoder creation; native control setup and input I/O excluded.',
                  steady_state_scope='In-process public encode API, owning output drop included in Rust; no decode, I/O or fingerprint hashing.',
                  memory_scope='encoder_object_bytes is Rust sizeof excluding heap vs native advertised size (overestimates restricted applications); maxrss is entire child including corpus loading and untimed validation, not codec-only heap.',
                  limitations=['No perceptual or SOTA claim.','No SIMD/best-available or other-competitor claim.','Two recordings are not a representative quality corpus.','Pristine native encoder; Opura retains its disclosed Q24 spectral-patch correction.'], cases={}, order=[])
    save = lambda: write_report(args.output/'report.json', report)
    affinity = os.sched_getaffinity(0)
    cpu = args.cpu if args.cpu is not None else min(affinity)
    if cpu not in affinity:
        raise ValueError('CPU outside allowed affinity')
    try:
        os.sched_setaffinity(0, {cpu});report['hardware_before']=snapshot(cpu);save()
        workloads = cases(args.profile, args.corpus)
        for name, settings, pcm, source_hash in workloads:
            folder=args.output/name;folder.mkdir()
            path=folder/'input.bin';case=prepare(path,settings,pcm)
            case.update(source_recording_sha256=source_hash, preflight={}, samples={'opura':[],'libopus':[]}, calibration={})
            report['cases'][name]=case;save()
            for impl,binary in binaries.items():
                record=folder/(impl+'.bin')
                sample=invoke(binary,path,case,1,args.warmup,record,folder/'rss.txt')
                validation_record(record,case,sample)
                case['preflight'][impl]=sample
            for impl in binaries:
                other='libopus' if impl=='opura' else 'opura'
                verify(binaries[other],path,folder/(impl+'.bin'),case['frames'])
            case['packet_diagnostics']={impl:verify(binaries['libopus'],path,folder/(impl+'.bin'),case['frames']) for impl in binaries}
            case['cross_decoder_verified']=True
            case['validation_record_sha256']={impl:sha(folder/(impl+'.bin')) for impl in binaries}
            if not report.get('fault_sensitivity'):
                report['fault_sensitivity']=fault_probes(binaries,path,folder/'opura.bin',folder)
            for impl,sample in case['preflight'].items():
                count=min(1000,max(1,math.ceil(args.minimum_ms*1250000/sample['elapsed_ns'])))
                case['calibration'][impl]=count
            save();print(name,'cross-decoder verified',flush=True)
        report['hardware_setup']=report['hardware_before'];report['hardware_before']=snapshot(cpu)
        if args.preflight_only:
            if fingerprint_sources()!=digest or any(sha(v)!=report['binary_sha256'][k] for k,v in binaries.items()):raise ValueError('Sources/binaries changed during preflight')
            report['status']='preflight_pass';report['baseline_complete']=False;save();return
        planned=schedule(list(report['cases']),args.rounds)
        for index,row in enumerate(planned):
            name=row['case'];case=report['cases'][name];folder=args.output/name
            report['order'].append(row)
            for impl in row['implementations']:
                sample=invoke(binaries[impl],folder/'input.bin',case,case['calibration'][impl],args.warmup,None,folder/'rss.txt')
                check_sample(sample,case,case['calibration'][impl],case['preflight'][impl])
                case['samples'][impl].append(sample)
            save()
            if (index+1)%len(report['cases'])==0:
                print('round',row['round']+1,'complete',flush=True)
        report['hardware_after']=snapshot(cpu);summarize(report);report['status']='pass';report['baseline_complete']=args.profile=='full' and args.rounds>=15
        if fingerprint_sources()!=digest or any(sha(v)!=report['binary_sha256'][k] for k,v in binaries.items()):raise ValueError('Sources/binaries changed during measurements')
        if subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()!=report['base_revision']:raise ValueError('Repository revision changed during measurement')
        validate_report(report);save()
    except BaseException as error:
        report['status']='failed';report['error']=str(error);save();raise
    finally:
        os.sched_setaffinity(0,affinity)


if __name__ == '__main__':
    main()
