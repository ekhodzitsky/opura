#!/usr/bin/env python3
"""Optional official RFC8251 decoder matrix; external opus_compare only, no Cargo dependencies.

Usage: python3 tools/check_conformance.py /path/to/pinned/opus archive.tar.gz
The full default matrix is12 streams x5 rates x2 channels. Filters produce an
explicitly incomplete report and cannot establish the full conformance result.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import struct
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
RATES = (8000, 12000, 16000, 24000, 48000)
CHANNELS = (1, 2)
MAX_ARCHIVE = 128 * 1024 * 1024
MAX_MEMBER = 64 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
MANIFEST = json.loads((ROOT / 'tools/conformance_manifest.json').read_text())


def digest_file(path, algorithm='sha256'):
    digest = hashlib.new(algorithm)
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(65536), b''):
            digest.update(chunk)
    return digest.hexdigest()


def extract_verified(archive, destination, manifest=MANIFEST):
    """Validate names/types/bounds before writes; never extract tar paths or links."""
    archive, destination = Path(archive), Path(destination)
    if archive.stat().st_size > MAX_ARCHIVE:
        raise ValueError('Archive exceeds128MiB')
    if digest_file(archive) != manifest['archive_sha256']:
        raise ValueError('Official archive SHA256 mismatch')
    expected = manifest['sha1']
    with tarfile.open(archive, 'r:gz') as tar:
        members = {}
        total = 0
        for index, member in enumerate(tar):
            if index > len(expected):
                raise ValueError('Too many archive members')
            if member.isdir() and member.name.rstrip('/') == 'opus_newvectors':
                continue
            name = PurePosixPath(member.name)
            if (not member.isfile() or len(name.parts) != 2
                    or name.parts[0] != 'opus_newvectors' or name.parts[1] not in expected
                    or member.name != str(name)):
                raise ValueError('Unexpected archive name/type: ' + member.name)
            key = name.parts[1]
            if key in members:
                raise ValueError('Duplicate archive member: ' + key)
            if not 0 < member.size <= MAX_MEMBER:
                raise ValueError('Archive member size exceeds bound: ' + key)
            total += member.size
            if total > MAX_TOTAL:
                raise ValueError('Expanded archive exceeds512MiB')
            members[key] = member
        if set(members) != set(expected):
            raise ValueError('Official archive file set mismatch')
        destination.mkdir(parents=True, exist_ok=True)
        for name, member in sorted(members.items()):
            extracted = tar.extractfile(member)
            if extracted is None:
                raise ValueError('Missing archive data: ' + name)
            partial = destination / (name + '.partial')
            try:
                digest = hashlib.sha1()
                remaining = member.size
                with extracted, partial.open('xb') as output:
                    while remaining:
                        chunk = extracted.read(min(65536, remaining))
                        if not chunk:
                            raise ValueError('Truncated archive member: ' + name)
                        digest.update(chunk)
                        output.write(chunk)
                        remaining -= len(chunk)
                if digest.hexdigest() != expected[name]:
                    raise ValueError('RFC8251 SHA1 mismatch: ' + name)
                target = destination / name
                if target.exists() or target.is_symlink():
                    raise ValueError('Extraction target already exists: ' + name)
                partial.rename(target)
            except BaseException:
                partial.unlink(missing_ok=True)
                raise
    return total


def vector_counts(path):
    """Independent big-endian framing/accounting check for the Rust runner stats."""
    packets = checks = 0
    with Path(path).open('rb') as stream:
        while header := stream.read(8):
            if len(header) != 8:
                raise ValueError('Truncated vector header')
            length, final_range = struct.unpack('>II', header)
            if length > 65536 or len(stream.read(length)) != length:
                raise ValueError('Invalid/truncated vector payload')
            packets += 1
            checks += int(final_range != 0)
            if packets > 1_000_000:
                raise ValueError('Vector packet count exceeds bound')
    if not packets:
        raise ValueError('Empty vector')
    return packets, checks


def metric_result(result):
    text = result.stdout + result.stderr
    match = re.search(r'Opus quality metric: ([^\s]+) %', text)
    quality = float(match[1]) if match else None
    passed = (result.returncode == 0 and 'Test vector PASSES' in text
              and quality is not None and math.isfinite(quality) and 0 <= quality <= 100)
    return {'passed': passed, 'quality_percent': quality if quality is not None and math.isfinite(quality) else None}


def source_fingerprint():
    files = [*ROOT.glob('src/**/*.rs'), ROOT / 'examples/decode_vectors.rs',
             ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'rust-toolchain.toml',
             ROOT / 'tools/check_conformance.py', ROOT / 'tools/conformance_manifest.json']
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(str(path.relative_to(ROOT)).encode() + b'\0')
        digest.update(path.read_bytes())
        digest.update(b'\0')
    return digest.hexdigest()


def write_report(path, report):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile('w', dir=path.parent, prefix='.conformance-', delete=False) as output:
            temporary = Path(output.name)
            json.dump(report, output, indent=2, sort_keys=True, allow_nan=False)
            output.write('\n')
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def run_case(case, binary, compare, vectors, counts, work):
    number, rate, channels = case
    name = f'testvector{number:02}'
    record = {'vector': number, 'rate': rate, 'channels': channels, 'passed': False}
    with tempfile.TemporaryDirectory(prefix=f'{name}-{rate}-{channels}-', dir=work) as temporary:
        output = Path(temporary) / 'decoded.pcm'
        process = subprocess.run([str(binary), str(rate), str(channels), str(vectors / (name + '.bit')), str(output)],
                                 capture_output=True, text=True, timeout=180)
        if process.returncode:
            record['error'] = f'decode exit{process.returncode}: ' + process.stderr[-4000:].replace(temporary, '<temporary>')
            return record
        stats = json.loads(process.stdout)
        packets, checks = counts[number]
        if (stats.get('packets') != packets or stats.get('range_checks') != checks
                or not isinstance(stats.get('samples_per_channel'), int) or stats['samples_per_channel'] <= 0
                or output.stat().st_size != stats['samples_per_channel'] * channels * 2):
            record['error'] = 'Decoder packet/range/sample accounting mismatch'
            return record
        record.update(stats)
        record['pcm_sha256'] = digest_file(output)
        variants = {}
        for suffix in ['', 'm']:
            arguments = [str(compare)] + (['-s'] if channels == 2 else [])
            arguments += ['-r', str(rate), str(vectors / (name + suffix + '.dec')), str(output)]
            result = subprocess.run(arguments, capture_output=True, text=True, timeout=180)
            variants['normal' if not suffix else 'no_phase_inversion'] = metric_result(result)
        record['reference_variants'] = variants
        record['passed'] = any(v['passed'] for v in variants.values())
        if not record['passed']:
            record['error'] = 'Original conformance metric rejects both official outputs'
    return record


def probe_gates(binary, compare, vectors, work):
    """Prove live range and quality gates reject corrupted external artifacts."""
    corrupt = work / 'wrong-range.bit'
    shutil.copyfile(vectors / 'testvector01.bit', corrupt)
    packet = 0
    with corrupt.open('r+b') as stream:
        while header := stream.read(8):
            if len(header) != 8:
                raise ValueError('Truncated fault-probe header')
            length, final_range = struct.unpack('>II', header)
            if final_range:
                stream.seek(-4, os.SEEK_CUR)
                stream.write(struct.pack('>I', final_range ^ 1))
                break
            stream.seek(length, os.SEEK_CUR)
            packet += 1
        else:
            raise ValueError('Fault probe has no encoded range')
    result = subprocess.run([str(binary), '48000', '2', str(corrupt), str(work / 'wrong-range.pcm')],
                            capture_output=True, text=True, timeout=180)
    if result.returncode == 0 or 'range mismatch' not in result.stderr:
        raise ValueError('Live range gate failed to reject a corrupted diagnostic')
    pcm = work / 'zeroed.pcm'
    result = subprocess.run([str(binary), '48000', '2', str(vectors / 'testvector01.bit'), str(pcm)],
                            capture_output=True, text=True, timeout=180)
    if result.returncode:
        raise ValueError('Fault probe could not decode official vector01')
    length = pcm.stat().st_size
    with pcm.open('wb') as stream:
        remaining = length
        while remaining:
            chunk = min(remaining, 65536)
            stream.write(bytes(chunk))
            remaining -= chunk
    rejected = []
    for suffix in ['', 'm']:
        result = subprocess.run([str(compare), '-s', '-r', '48000', str(vectors / ('testvector01' + suffix + '.dec')), str(pcm)],
                                capture_output=True, text=True, timeout=180)
        rejected.append(not metric_result(result)['passed'])
    if not all(rejected):
        raise ValueError('Live quality gate failed to reject zeroed PCM')
    return {'corrupt_range_rejected': True, 'corrupted_packet_index': packet,
            'zero_pcm_rejected_by_both_references': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--report', type=Path, default=ROOT / 'target/conformance-report.json')
    parser.add_argument('--rate', type=int, choices=RATES, action='append')
    parser.add_argument('--channels', type=int, choices=CHANNELS, action='append')
    parser.add_argument('--vector', type=int, choices=range(1, 13), action='append')
    parser.add_argument('--jobs', type=int, choices=range(1, 9), default=2)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Expected pinned clean Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be clean')
    rates = sorted(set(args.rate or RATES))
    channels = sorted(set(args.channels or CHANNELS))
    numbers = sorted(set(args.vector or range(1, 13)))
    cases = [(number, rate, channel) for number in numbers for rate in rates for channel in channels]
    compiler = subprocess.check_output(['cc', '--version'], text=True).splitlines()[0]
    rustc = subprocess.check_output(['rustc', '--version', '--verbose'], cwd=ROOT, text=True)
    host = next(line.removeprefix('host: ') for line in rustc.splitlines() if line.startswith('host: '))
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    report = {'schema_version': 1, 'complete': False, 'passed': False, 'opura_version': version,
              'source_sha256': source_fingerprint(), 'scope': 'classic Opus signed16; no QEXT/neural/OSCE paths',
              'archive_url': MANIFEST['archive_url'], 'archive_sha256': MANIFEST['archive_sha256'],
              'file_hash_source': MANIFEST['hashes_source'], 'reference_revision': REVISION,
              'compare_source_sha256': digest_file(source / 'src/opus_compare.c'),
              'compiler': compiler, 'compare_flags': ['-O2', '-std=c99', '-lm'],
              'rustc': rustc.strip(), 'profile': 'release', 'rates': rates, 'channels': channels,
              'vectors': numbers, 'results': []}
    results = []
    try:
        (ROOT / 'target').mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='opura-conformance-', dir=ROOT / 'target') as temporary:
            work = Path(temporary)
            vectors = work / 'vectors'
            report['expanded_bytes'] = extract_verified(args.archive, vectors)
            counts = {number: vector_counts(vectors / f'testvector{number:02}.bit') for number in numbers}
            compare = work / 'opus_compare'
            subprocess.run(['cc', '-O2', '-std=c99', str(source / 'src/opus_compare.c'), '-lm', '-o', str(compare)], check=True)
            build = ROOT / 'target/conformance-build'
            environment = os.environ.copy()
            for name in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS']:
                environment.pop(name, None)
            subprocess.run(['cargo', 'test', '--offline', '--locked', '--example', 'decode_vectors', '--target-dir', str(build), '--target', host], cwd=ROOT, env=environment, check=True)
            subprocess.run(['cargo', 'build', '--offline', '--locked', '--release', '--example', 'decode_vectors', '--target-dir', str(build), '--target', host], cwd=ROOT, env=environment, check=True)
            binary = build / host / 'release/examples/decode_vectors'
            with ThreadPoolExecutor(max_workers=args.jobs) as pool:
                pending = {pool.submit(run_case, case, binary, compare, vectors, counts, work): case for case in cases}
                for future in as_completed(pending):
                    result = future.result()
                    results.append(result)
                    print(f'{"PASS" if result["passed"] else "FAIL"} vector{result["vector"]:02} {result["rate"]}Hz C{result["channels"]}'
                          + (' ' + result['error'] if 'error' in result else ''), flush=True)
            report['fault_sensitivity'] = probe_gates(binary, compare, vectors, work)
            report['results'] = sorted(results, key=lambda r: (r['vector'], r['rate'], r['channels']))
            report['complete'] = len(cases) == 120 and len(results) == 120
            report['passed'] = report['complete'] and all(r['passed'] for r in results)
    except (OSError, ValueError, tarfile.TarError, subprocess.SubprocessError) as error:
        report['error'] = str(error).replace(str(ROOT), '<workspace>')
        report['results'] = sorted(results, key=lambda r: (r['vector'], r['rate'], r['channels']))
        write_report(args.report, report)
        raise
    write_report(args.report, report)
    if not all(r['passed'] for r in results):
        raise SystemExit('Conformance failed; see report')
    if not report['complete']:
        print('Subset passed; report is incomplete and does not establish full conformance.', flush=True)
    else:
        print('All120 official conformance cases and encoded final ranges passed.', flush=True)


if __name__ == '__main__':
    main()
