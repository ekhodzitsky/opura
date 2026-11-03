#!/usr/bin/env python3
"""Optional matched scalar fixed-point decoder benchmark; no foreign Cargo dependency."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import re
import statistics
import subprocess
import tempfile
import tomllib

from check_conformance import ROOT, REVISION, MANIFEST, RATES, CHANNELS, extract_verified, digest_file, write_report, vector_counts
from competitive_gate import upper_bound
import check_conformance as conformance

CASES = tuple((rate, channels) for rate in RATES for channels in CHANNELS)
PACKETS_PER_PASS = 20075
MODE_PACKETS = [4526, 4491, 11058]
SAMPLES48_PER_PASS = 15390480
CFLAGS = ['-O3', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API']

CFLAGS_V2 = CFLAGS + ['-march=x86-64', '-mtune=generic']
RUST_PROFILE = {'OPT_LEVEL': '3', 'CODEGEN_UNITS': '16', 'LTO': 'false', 'DEBUG': '0',
                'DEBUG_ASSERTIONS': 'false', 'OVERFLOW_CHECKS': 'false', 'PANIC': 'unwind', 'INCREMENTAL': 'false'}
RUST_FLAGS = '-C target-cpu=x86-64'
PROBES = [name + '_' + kind for name in ['rust', 'native'] for kind in ['wrong_range', 'wrong_pcm', 'minimum_extension', 'pass_bound', 'legacy_cli']]


def check_timed(sample, packets, minimum_passes, minimum_ms):
    if type(minimum_passes) is not int or not 1 <= minimum_passes <= 1000:
        raise ValueError('Invalid minimum pass count')
    if type(minimum_ms) is not int or not 0 <= minimum_ms <= 2000:
        raise ValueError('Invalid minimum time')
    if not isinstance(sample, dict):
        raise ValueError('Missing timed sample')
    check_sample(sample, packets, sample.get('passes'))
    if sample['mode_packets'] != MODE_PACKETS:
        raise ValueError('Official mode counts changed')
    if sample['passes'] < minimum_passes or sample['elapsed_ns'] < minimum_ms * 1000000:
        raise ValueError('Incomplete passes or full minimum time')
    if type(sample.get('init_ns')) is not int or sample['init_ns'] <= 0:
        raise ValueError('Missing initialization timing')
    return sample


def summarize_v2(report):
    # Reuse the before/after track's paired percentile interval without a
    # module-level import cycle; both tools share decoder adapter validation.
    from decoder_change import interval
    rng = random.Random(62482)
    cases = {}
    values = {}
    for name, case in sorted(report['cases'].items()):
        values[name] = [[s['elapsed_ns'] / s['passes'] for s in case['samples'][k]]
                        for k in ['opura', 'libopus']]
        a, b = values[name]
        cases[name] = interval(b, a, rng)
    point = math.exp(statistics.mean(math.log(c['ratio']) for c in cases.values()))
    bootstrap = []
    for _ in range(2000):
        indices = [rng.randrange(report['rounds']) for _ in range(report['rounds'])]
        bootstrap.append(math.exp(statistics.mean(
            math.log(statistics.median([a[i] for i in indices]) /
                     statistics.median([b[i] for i in indices])) for a, b in values.values())))
    bootstrap.sort()
    return {'cases': cases, 'aggregate': {'ratio': point, 'ci95': [bootstrap[49], bootstrap[1949]]}}


def fingerprint_sources():
    files = [*ROOT.glob('src/**/*.rs'), ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'rust-toolchain.toml',
             ROOT / 'examples/decoder_bench.rs', ROOT / 'examples/decode_vectors.rs',
             *[ROOT / 'tools' / name for name in ['decoder_bench.py', 'check_conformance.py', 'conformance_manifest.json', 'competitive_gate.py', 'packet_bench.py', 'decoder_change.py']],
             ROOT / 'tools/benchmarks/decoder.c']
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(str(path.relative_to(ROOT)).encode() + b'\0' + path.read_bytes() + b'\0')
    return digest.hexdigest()


def check_sample(sample, packets, passes):
    if type(packets) is not int or packets <= 0 or type(passes) is not int or not 1 <= passes <= 1000:
        raise ValueError('Invalid expected packet/pass count')
    if (not isinstance(sample, dict) or type(sample.get('packets')) is not int or type(sample.get('passes')) is not int
            or sample.get('packets') != packets or sample.get('passes') != passes):
        raise ValueError('Adapter packet/pass count mismatch')
    for key in ['samples_per_pass', 'decoder_bytes', 'elapsed_ns']:
        if type(sample.get(key)) is not int or sample[key] <= 0:
            raise ValueError('Invalid adapter field: ' + key)
    if not re.fullmatch('[0-9a-f]{16}', str(sample.get('fingerprint', ''))):
        raise ValueError('Missing full PCM/range fingerprint')
    latency = sample.get('latency_ns', {})
    if (not isinstance(latency, dict) or set(latency) != {'p50', 'p95', 'p99', 'max'}
            or any(type(v) is not int or v <= 0 for v in latency.values())
            or not latency['p50'] <= latency['p95'] <= latency['p99'] <= latency['max']):
        raise ValueError('Invalid latency distribution')
    counts, totals = sample.get('mode_packets'), sample.get('mode_latency_total_ns')
    if (not isinstance(counts, list) or not isinstance(totals, list) or len(counts) != 3 or len(totals) != 3
            or any(type(v) is not int or v < 0 for v in counts + totals) or sum(counts) != packets
            or any((count == 0) != (total == 0) for count, total in zip(counts, totals))
            or not latency['p50'] * (packets // 2) <= sum(totals) <= latency['max'] * packets):
        raise ValueError('Invalid mode latency accounting')
    return sample


def compare_samples(a, b):
    for key in ['packets', 'samples_per_pass', 'fingerprint', 'mode_packets']:
        if a[key] != b[key]:
            raise ValueError('Rust/reference correctness mismatch: ' + key)


def conformance_cases(report):
    if (not isinstance(report, dict) or type(report.get('schema_version')) is not int or report.get('schema_version') != 1 or report.get('complete') is not True
            or report.get('passed') is not True or report.get('archive_sha256') != MANIFEST['archive_sha256']):
        raise ValueError('Complete verified conformance report required')
    results = report.get('results')
    if not isinstance(results, list) or len(results) != 120:
        raise ValueError('Missing conformance cases')
    indexed = {}
    for case in results:
        if (not isinstance(case, dict) or type(case.get('vector')) is not int or case['vector'] not in range(1,13)
                or type(case.get('rate')) is not int or case['rate'] not in RATES
                or type(case.get('channels')) is not int or case['channels'] not in CHANNELS):
            raise ValueError('Invalid conformance geometry')
        key = (case.get('vector'), case.get('rate'), case.get('channels'))
        if (key in indexed or case.get('passed') is not True
                or not re.fullmatch('[0-9a-f]{64}', str(case.get('pcm_sha256', '')))
                or any(type(case.get(k)) is not int or case[k] <= 0 for k in ['packets', 'range_checks', 'samples_per_channel'])
                or case['packets'] != case['range_checks']):
            raise ValueError('Invalid conformance case')
        indexed[key] = case
    expected = {(number, rate, channels) for number in range(1, 13) for rate, channels in CASES}
    if set(indexed) != expected:
        raise ValueError('Conformance matrix mismatch')
    return indexed


def schedule(rounds):
    rng = random.Random(6716)
    result = []
    for round_number in range(rounds):
        cases = list(CASES)
        rng.shuffle(cases)
        for rate, channels in cases:
            implementations = ['opura','libopus']
            rng.shuffle(implementations)
            result.append({'round':round_number,'case':f'{rate}/{channels}','implementations':implementations})
    return result


def evidence_errors(report):
    if not isinstance(report, dict) or type(report.get('schema_version')) is not int or report.get('schema_version') not in [1, 2] or report.get('track') != 'decoder_scalar':
        return ['Missing scalar decoder report schema/track.']
    if report.get('correct') is not True or report.get('packets_per_pass') != PACKETS_PER_PASS or report.get('cflags') != (CFLAGS if report.get('schema_version') == 1 else CFLAGS_V2):
        return ['Missing correctness or matched workload/profile evidence.']
    if report.get('reference_revision') != REVISION or report.get('archive_sha256') != MANIFEST['archive_sha256']:
        return ['Reference/corpus provenance mismatch.']
    for key in ['source_sha256', 'corpus_sha256', 'rust_binary_sha256', 'reference_binary_sha256', 'conformance_report_sha256']:
        if not re.fullmatch('[0-9a-f]{64}', str(report.get(key, ''))):
            return ['Missing provenance: ' + key]
    if report['schema_version'] == 2:
        if report.get('status') != 'complete' or report.get('rust_profile') != RUST_PROFILE or report.get('rust_flags') != RUST_FLAGS:
            return ['Missing complete matched build profile']
        if report.get('fault_sensitivity') != {k: ('verified' if k.endswith(('minimum_extension', 'legacy_cli')) else 'rejected') for k in PROBES}:
            return ['Missing live wrong range/PCM rejection probes']
        if not isinstance(report.get('rustc'), str) or not report['rustc'].startswith('rustc ') or not isinstance(report.get('cc'), str) or not report['cc']:
            return ['Missing compiler identity']
    rounds = report.get('rounds')
    if type(rounds) is not int or not 15 <= rounds <= 100 or report.get('conformance_pcm_hashes_verified') != 120:
        return ['At least15 paired rounds and120 conformance PCM hashes are required.']
    if (type(report.get('warmup_passes')) is not int or not 1 <= report['warmup_passes'] <= 100 or report.get('seed') != 6716):
        return ['Missing warmup/seed protocol evidence.']
    if report.get('order') != schedule(rounds):
        return ['Missing or changed randomized paired schedule.']
    before, after = report.get('hardware_before'), report.get('hardware_after')
    if (not isinstance(before, dict) or not isinstance(after, dict) or type(before.get('cpu')) is not int
            or before['cpu'] < 0 or after.get('cpu') != before['cpu']):
        return ['Missing or inconsistent CPU affinity evidence.']
    if type(after.get('cpu')) is not int:
        return ['Invalid after CPU affinity type']
    if report['schema_version'] == 2:
        for key in ['cpu_model', 'machine', 'kernel']:
            if not isinstance(before.get(key), str) or not before[key] or after.get(key) != before[key]:
                return ['Missing/changed hardware identity: ' + key]
    minimum_ms = report.get('minimum_sample_ms')
    if type(minimum_ms) is not int or not 50 <= minimum_ms <= 2000:
        return ['Invalid sampling window.']
    cases = report.get('cases')
    if not isinstance(cases, dict) or set(cases) != {f'{r}/{c}' for r, c in CASES}:
        return ['Missing/unexpected rate/channel cases.']
    errors = []
    for name, case in cases.items():
        if not isinstance(case, dict) or case.get('correct') is not True:
            errors.append(name + ': missing correctness verification.')
            continue
        if not isinstance(case.get('samples'), dict) or not isinstance(case.get('calibration'), dict):
            errors.append(name + ': missing sample/calibration objects.')
            continue
        a, b = case['samples'].get('opura'), case['samples'].get('libopus')
        if not isinstance(a, list) or not isinstance(b, list) or len(a) != rounds or len(b) != rounds:
            errors.append(name + ': missing paired samples.')
            continue
        try:
            rate = int(name.split('/')[0])
            calibration = case['calibration']
            for implementation in ['opura','libopus']:
                check_sample(calibration[implementation], PACKETS_PER_PASS, 1)
            compare_samples(calibration['opura'], calibration['libopus'])
            if report['schema_version'] == 2:
                for sample in calibration.values():
                    check_timed(sample, PACKETS_PER_PASS, 1, 0)
            for x, y in zip(a, b):
                if report['schema_version'] == 1:
                    check_sample(x, report['packets_per_pass'], case['passes'])
                    check_sample(y, report['packets_per_pass'], case['passes'])
                else:
                    check_timed(x, report['packets_per_pass'], case['passes'], minimum_ms)
                    check_timed(y, report['packets_per_pass'], case['passes'], minimum_ms)
                compare_samples(x, y)
                compare_samples(x, a[0])
                if x['samples_per_pass'] != SAMPLES48_PER_PASS * rate // 48000:
                    raise ValueError('Decoded duration mismatch')
                for sample, implementation in [(x,'opura'),(y,'libopus')]:
                    compare_samples(sample,calibration[implementation])
                    if (sample['decoder_bytes'] != calibration[implementation]['decoder_bytes']
                            or sample['elapsed_ns'] < minimum_ms * 1000000):
                        raise ValueError('State size changed or timing window too short')
        except (ValueError, KeyError, TypeError) as error:
            errors.append(name + ': ' + str(error))
    if not errors and report['schema_version'] == 2:
        try:
            probes = report['probe_samples']
            if set(probes) != {'rust', 'native', 'rust_legacy', 'native_legacy'}:
                raise ValueError('Missing live adapter probe samples')
            for name, implementation in [('rust', 'opura'), ('native', 'libopus')]:
                check_timed(probes[name], PACKETS_PER_PASS, 2, 2000)
                check_timed(probes[name + '_legacy'], PACKETS_PER_PASS, 1, 0)
                check_sample(probes[name + '_legacy'], PACKETS_PER_PASS, 1)
                for sample in [probes[name], probes[name + '_legacy']]:
                    compare_samples(sample, cases['48000/2']['calibration'][implementation])
        except (ValueError, KeyError, TypeError) as error:
            errors.append('Invalid live adapter probe evidence: ' + str(error))
    if not errors and report['schema_version'] == 2:
        if report.get('summary') != summarize_v2(report):
            errors.append('Missing or changed paired summary')
    return errors


def evaluate(report):
    errors = evidence_errors(report)
    if errors:
        return errors
    if report['schema_version'] == 2:
        for name, case in report['summary']['cases'].items():
            if case['ci95'][1] > 1.05:
                errors.append(name + ': ratio95 upper exceeds1.050 regression limit.')
        if report['summary']['aggregate']['ci95'][1] > .9:
            errors.append('Equal-weight aggregate ratio95 upper exceeds0.900 reference target.')
        return errors
    rounds = report['rounds']
    cases = report['cases']
    rng = random.Random(6716)
    aggregates = [[] for _ in range(rounds)]
    for name, case in sorted(cases.items()):
        a = [s['elapsed_ns'] / s['passes'] for s in case['samples']['opura']]
        b = [s['elapsed_ns'] / s['passes'] for s in case['samples']['libopus']]
        upper = upper_bound(a, b, rng)
        if upper > 1.05:
            errors.append(f'{name}: ratio95 upper {upper:.3f} exceeds1.050 regression limit.')
        for values, x, y in zip(aggregates, a, b):
            values.append(math.log(x / y))
    values = [math.exp(statistics.mean(row)) for row in aggregates]
    upper = upper_bound(values, [1.0] * rounds, rng)
    if upper > .9:
        errors.append(f'Equal-weight aggregate ratio95 upper {upper:.3f} exceeds0.900 reference target.')
    return errors


def optional_text(path):
    try:
        return Path(path).read_text().strip()
    except OSError as error:
        return 'unavailable: ' + str(error.strerror)


def hardware(cpu):
    model = next((line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')), 'unknown')
    root = Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq')
    return {'cpu_model': model, 'machine': platform.machine(), 'kernel': platform.release(), 'cpu': cpu,
            'governor': optional_text(root / 'scaling_governor'), 'scaling_driver': optional_text(root / 'scaling_driver'),
            'frequency_khz': optional_text(root / 'scaling_cur_freq'), 'frequency_locked': False, 'exclusive_cpu': False}


def compile_adapters(source, build, environment):
    def manifest(file, variable):
        match = re.search(r'^' + variable + r' = (.*?)(?=\n\n|\Z)', (source / file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing reference manifest: ' + variable)
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED')
             + manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    reference = build / 'libopus-scalar'
    result = subprocess.run(['cc', *CFLAGS_V2, *['-I' + str(source / p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                             str(ROOT / 'tools/benchmarks/decoder.c'), *[str(source / f) for f in files], '-lm', '-o', str(reference)],
                            capture_output=True, text=True)
    if result.stderr:
        print(result.stderr.replace(str(source), '<reference>').replace(str(ROOT), '<workspace>'), flush=True)
    result.check_returncode()
    rustc = subprocess.check_output(['rustc', '--version', '--verbose'], cwd=ROOT, text=True)
    host = next(l.removeprefix('host: ') for l in rustc.splitlines() if l.startswith('host: '))
    subprocess.run(['cargo', 'build', '--offline', '--locked', '--release', '--example', 'decoder_bench', '--example', 'decode_vectors', '--target-dir', str(build / 'rust'), '--target', host],
                   cwd=ROOT, env=environment, check=True)
    directory = build / 'rust' / host / 'release/examples'
    return directory / 'decoder_bench', directory / 'decode_vectors', reference, rustc


def invoke(binary, vectors, rate, channels, passes, warmup, packets, minimum_ms=0, expected=None):
    result = subprocess.run([str(binary), str(vectors), str(rate), str(channels), str(passes), str(warmup), str(minimum_ms), *([expected] if expected is not None else [])],
                            capture_output=True, text=True, timeout=600)
    if result.returncode:
        raise ValueError('Adapter failed: ' + result.stderr[-2000:])
    return check_timed(json.loads(result.stdout), packets, passes, minimum_ms)


def rejection_probes(rust, reference, vectors, work, expected, details):
    damaged = work / 'wrong-range'
    damaged.mkdir()
    for source in vectors.glob('testvector*.bit'):
        if source.name != 'testvector01.bit':
            (damaged / source.name).symlink_to(source.resolve())
    data = bytearray((vectors / 'testvector01.bit').read_bytes())
    data[4:8] = (int.from_bytes(data[4:8], 'big') ^ 0xffffffff).to_bytes(4, 'big')
    (damaged / 'testvector01.bit').write_bytes(data)
    result = {}
    for name, binary in [('rust', rust), ('native', reference)]:
        for kind, path, fingerprint, marker in [('wrong_range', damaged, expected, 'range mismatch'),
                                               ('wrong_pcm', vectors, f'{int(expected,16)^1:016x}', 'fingerprint mismatch')]:
            command = [str(binary), str(path), '48000', '2', '1', '1', '0', fingerprint]
            trial = subprocess.run(command, capture_output=True, text=True, timeout=180)
            # Native wording also names an encoded range mismatch.
            if trial.returncode == 0 or marker not in trial.stderr:
                raise ValueError('Rejection probe failed: ' + name + '_' + kind)
            result[name + '_' + kind] = 'rejected'
    tiny = work / 'pass-bound'
    tiny.mkdir()
    for number in range(1,13):
        (tiny / f'testvector{number:02}.bit').write_bytes(b'\x00\x00\x00\x01\x00\x00\x00\x00\xfc')
    for name, binary in [('rust', rust), ('native', reference)]:
        extended = invoke(binary, vectors, 48000, 2, 1, 1, PACKETS_PER_PASS, 2000, expected)
        if extended['passes'] <= 1:
            raise ValueError('Minimum extension not exercised')
        details[name] = extended
        result[name + '_minimum_extension'] = 'verified'
        legacy = subprocess.run([str(binary), str(vectors), '48000', '2', '1', '1'], capture_output=True, text=True, timeout=180)
        legacy.check_returncode()
        old = check_sample(json.loads(legacy.stdout), PACKETS_PER_PASS, 1)
        if old['fingerprint'] != expected:
            raise ValueError('Legacy CLI output changed')
        details[name + '_legacy'] = old
        result[name + '_legacy_cli'] = 'verified'
        bounded = subprocess.run([str(binary), str(tiny), '48000', '2', '1', '1', '2000'], capture_output=True, text=True, timeout=30)
        if bounded.returncode == 0 or 'minimum time exceeds1000pass bound' not in bounded.stderr:
            raise ValueError('Pass bound rejection not exercised')
        result[name + '_pass_bound'] = 'rejected'
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path, nargs='?')
    parser.add_argument('archive', type=Path, nargs='?')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--conformance', type=Path, default=ROOT / 'docs/validation/results/decoder-current-0.2.3-conformance.json')
    parser.add_argument('--rounds', type=int, default=15)
    parser.add_argument('--warmup', type=int, default=1)
    parser.add_argument('--minimum-ms', type=int, default=200)
    parser.add_argument('--cpu', type=int)
    parser.add_argument('--check', type=Path)
    parser.add_argument('--preflight-only', action='store_true')
    args = parser.parse_args()
    if args.check:
        checked_report = json.loads(args.check.read_text())
        if checked_report.get('schema_version') == 2 and checked_report.get('source_sha256') != fingerprint_sources():
            raise ValueError('Stale benchmark source; use the recorded milestone checkout')
        errors = evaluate(checked_report)
        for error in errors:
            print(error)
        print('Scalar reference target ' + ('FAILS' if errors else 'PASSES') + '; whole-market superiority is unmeasured.')
        raise SystemExit(bool(errors))
    if not args.source or not args.archive or not args.out:
        parser.error('source, archive and --out are required')
    if not 15 <= args.rounds <= 100 or not 1 <= args.warmup <= 100 or not 50 <= args.minimum_ms <= 2000:
        parser.error('invalid rounds/warmup/minimum-ms bounds')
    if args.out.exists() or args.out.is_symlink() or args.out.with_suffix('.partial.json').exists() or args.out.with_suffix('.partial.json').is_symlink():
        parser.error('output exists; preserve prior evidence')
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Pinned reference revision required')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be clean')
    conformance_report = json.loads(args.conformance.read_text())
    if conformance_report.get('source_sha256') != conformance.source_fingerprint():
        raise ValueError('Current source-bound conformance report required')
    baseline = conformance_cases(conformance_report)
    digest = fingerprint_sources()
    build = ROOT / 'target/decoder-bench' / digest[:16]
    build.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    for key in list(environment):
        if key in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'CARGO_BUILD_RUSTFLAGS'] or key.startswith('CARGO_PROFILE_RELEASE_') or (key.startswith('CARGO_TARGET_') and key.endswith('_RUSTFLAGS')):
            environment.pop(key, None)
    if platform.machine() != 'x86_64':
        raise ValueError('This matched generic target profile requires x86_64')
    environment['RUSTFLAGS'] = RUST_FLAGS
    for key, value in RUST_PROFILE.items():
        environment['CARGO_PROFILE_RELEASE_' + key] = value
    rust, verifier, reference, rustc = compile_adapters(source, build, environment)
    original_affinity = os.sched_getaffinity(0)
    cpu = args.cpu if args.cpu is not None else min(original_affinity)
    if cpu not in original_affinity:
        raise SystemExit('CPU is outside allowed affinity')
    report = {'schema_version': 2, 'status': 'in_progress', 'rust_profile': RUST_PROFILE, 'rust_flags': RUST_FLAGS, 'track': 'decoder_scalar', 'opura_version': tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version'],
              'source_sha256': digest, 'base_revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'reference_revision': REVISION, 'archive_sha256': MANIFEST['archive_sha256'],
              'rust_binary_sha256': digest_file(rust), 'reference_binary_sha256': digest_file(reference),
              'conformance_report_sha256': digest_file(args.conformance), 'conformance_pcm_hashes_verified': 0,
              'reference_profile': 'scalar-dispatch fixed-point fast64; compiler auto-vectorization allowed', 'cflags': CFLAGS_V2,
              'rustc': rustc.strip(), 'cc': subprocess.check_output(['cc', '--version'], text=True).splitlines()[0],
              'rounds': args.rounds, 'warmup_passes': args.warmup, 'minimum_sample_ms': args.minimum_ms, 'seed': 6716,
              'created_utc': datetime.now(timezone.utc).isoformat(), 'cases': {}, 'order': [], 'correct': False}
    try:
        os.sched_setaffinity(0, {cpu})
        report['hardware_before'] = hardware(cpu)
        with tempfile.TemporaryDirectory(prefix='work-', dir=build) as temporary:
            work = Path(temporary)
            vectors = work / 'vectors'
            extract_verified(args.archive, vectors)
            packets = sum(vector_counts(vectors / f'testvector{n:02}.bit')[0] for n in range(1, 13))
            if packets != PACKETS_PER_PASS:
                raise ValueError('Official packet count mismatch')
            report['packets_per_pass'] = packets
            corpus = hashlib.sha256()
            for number in range(1, 13):
                corpus.update(f'testvector{number:02}.bit'.encode() + b'\0' + (vectors / f'testvector{number:02}.bit').read_bytes())
            report['corpus_sha256'] = corpus.hexdigest()
            for rate, channels in CASES:
                # Current public decode output must reproduce every accepted
                # conformance PCM hash, independently of the timing adapters.
                for number in range(1, 13):
                    output = work / f'verify-{number}-{rate}-{channels}.pcm'
                    result = subprocess.run([str(verifier), str(rate), str(channels), str(vectors / f'testvector{number:02}.bit'), str(output)],
                                            capture_output=True, text=True, timeout=180)
                    if result.returncode or digest_file(output) != baseline[number, rate, channels]['pcm_sha256']:
                        raise ValueError('Current decoder differs from accepted conformance PCM')
                    output.unlink()
                    report['conformance_pcm_hashes_verified'] += 1
                a = invoke(rust, vectors, rate, channels, 1, args.warmup, packets)
                b = invoke(reference, vectors, rate, channels, 1, args.warmup, packets)
                compare_samples(a, b)
                passes = min(1000, max(1, math.ceil(1.25 * args.minimum_ms * 1_000_000 / min(a['elapsed_ns'], b['elapsed_ns']))))
                name = f'{rate}/{channels}'
                report['cases'][name] = {'correct': True, 'passes': passes, 'calibration': {'opura': a, 'libopus': b}, 'samples': {'opura': [], 'libopus': []}}
                print(f'VERIFIED {name}: fingerprint{a["fingerprint"]}, passes{passes}', flush=True)
            report['probe_samples'] = {}
            report['fault_sensitivity'] = rejection_probes(rust, reference, vectors, work,
                                                         report['cases']['48000/2']['calibration']['opura']['fingerprint'], report['probe_samples'])
            if args.preflight_only:
                if fingerprint_sources() != digest:
                    raise ValueError('Source changed during preflight')
                report['status'] = 'preflight_only'
                write_report(args.out, report)
                print('Preflight passes; no retained timings or performance claim.', flush=True)
                return
            report['order'] = schedule(args.rounds)
            binaries = {'opura': rust, 'libopus': reference}
            for entry in report['order']:
                name = entry['case']
                rate, channels = map(int,name.split('/'))
                case = report['cases'][name]
                for implementation in entry['implementations']:
                    sample = invoke(binaries[implementation], vectors, rate, channels, case['passes'], args.warmup, packets, args.minimum_ms, case['calibration'][implementation]['fingerprint'])
                    compare_samples(sample, case['calibration'][implementation])
                    for binary_name, binary in binaries.items():
                        key = 'rust_binary_sha256' if binary_name == 'opura' else 'reference_binary_sha256'
                        if digest_file(binary) != report[key]:
                            raise ValueError('Measured binary changed')
                    if fingerprint_sources() != digest:
                        raise ValueError('Measured source changed')
                    case['samples'][implementation].append(sample)
                    write_report(args.out.with_suffix('.partial.json'), report)
                if sum(len(c['samples']['opura']) for c in report['cases'].values()) % len(CASES) == 0:
                    print(f'ROUND {entry["round"] + 1}/{args.rounds}', flush=True)
            report['hardware_after'] = hardware(cpu)
            report['correct'] = True
    finally:
        os.sched_setaffinity(0, original_affinity)
    report['status'] = 'complete'
    report['summary'] = summarize_v2(report)
    structural = evidence_errors(report)
    if structural:
        raise ValueError('Invalid complete evidence: ' + '; '.join(structural))
    report['reference_target_failures'] = evaluate(report)
    report['reference_target_met'] = not report['reference_target_failures']
    if fingerprint_sources() != digest:
        raise ValueError('Sources changed during benchmark; discard timings')
    if args.out.exists() or args.out.is_symlink():
        raise ValueError('Output appeared during run; refusing to overwrite')
    write_report(args.out, report)
    for name, case in sorted(report['cases'].items()):
        a = statistics.median(s['elapsed_ns'] / s['passes'] / packets for s in case['samples']['opura'])
        b = statistics.median(s['elapsed_ns'] / s['passes'] / packets for s in case['samples']['libopus'])
        print(f'{name} Opura{a:.1f}ns/packet reference{b:.1f}ns/packet ratio{a/b:.3f}', flush=True)
    print('Scalar reference target ' + ('PASSES' if report['reference_target_met'] else 'FAILS') + '; other competitors/SIMD remain unmeasured.', flush=True)


if __name__ == '__main__':
    main()
