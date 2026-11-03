#!/usr/bin/env python3
"""Optional independent native verification of actual mapped CLI output files.

Checks all standard rates,1..8channels, positional/unpositioned layouts,
applications, complete packet budgets and exact trimmed PCM. This is a generic
encoder interoperability check, not a perceptual quality or speed comparison.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import tempfile
import tomllib
from audio_baseline import CFLAGS, REVISION, compile_reference
from mapped_audio_fixtures import crc, decode, setup

ROOT = Path(__file__).resolve().parent.parent
MASKS = [4, 3, 7, 0x33, 0x37, 0x3f, 0x70f, 0x63f]


def packets(data):
    """Independent complete-file CRC, page sequence and packet reconstruction."""
    offset = 0
    pending = bytearray()
    result = []
    sequence = 0
    serial = None
    ended = False
    granule = 0
    while offset < len(data):
        if ended or len(data) - offset < 27:
            raise ValueError('Trailing/truncated Ogg data')
        head = bytearray(data[offset:offset + 27])
        if head[:5] != b'OggS\0' or head[5] & ~7:
            raise ValueError('Invalid Ogg capture/version/flags')
        flags = head[5]
        page_serial, page_sequence, checksum = struct.unpack_from('<III', head, 14)
        if page_sequence != sequence or (sequence == 0) != bool(flags & 2):
            raise ValueError('Ogg page sequence/BOS mismatch')
        if serial is not None and serial != page_serial:
            raise ValueError('Multiple Ogg serials')
        serial = page_serial
        if bool(flags & 1) != bool(pending):
            raise ValueError('Ogg continuation mismatch')
        begin = offset + 27
        laces = data[begin:begin + head[26]]
        if len(laces) != head[26]:
            raise ValueError('Truncated Ogg laces')
        begin += len(laces)
        end = begin + sum(laces)
        body = data[begin:end]
        head[22:26] = bytes(4)
        if end > len(data) or crc(head + laces + body) != checksum:
            raise ValueError('Truncated Ogg body or bad CRC')
        completed = 0
        cursor = 0
        for lace in laces:
            pending += body[cursor:cursor + lace]
            cursor += lace
            if len(pending) > 16 * 1024 * 1024:
                raise ValueError('Oversized Ogg packet')
            if lace < 255:
                result.append(bytes(pending))
                pending.clear()
                completed += 1
        position = struct.unpack_from('<Q', head, 6)[0]
        if completed:
            if position == 0xffffffffffffffff or position < granule:
                raise ValueError('Missing/backward Ogg granule')
            granule = position
        elif position != 0xffffffffffffffff:
            raise ValueError('Granule without completed packet')
        ended = bool(flags & 4)
        if ended and (pending or not completed):
            raise ValueError('Incomplete EOS packet')
        offset = end
        sequence += 1
    if not ended or pending or len(result) < 3:
        raise ValueError('Missing complete Ogg headers/audio/EOS')
    return result, granule


def wave(rate, channels, positioned, frames):
    values = [int(1800 * math.sin(2 * math.pi * (37 + 83 * channel) * frame / rate))
              for frame in range(frames) for channel in range(channels)]
    pcm = struct.pack('<' + 'h' * len(values), *values)
    fmt = struct.pack('<HHIIHHHHI', 0xfffe, channels, rate, rate * channels * 2,
                      channels * 2, 16, 22, 16, MASKS[channels - 1] if positioned else 0)
    fmt += bytes([1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113])
    return b'RIFF' + struct.pack('<I', 60 + len(pcm)) + b'WAVEfmt ' + struct.pack('<I', 40) + fmt + b'data' + struct.pack('<I', len(pcm)) + pcm


def wave_pcm(data, channels, positioned, frames):
    extended = channels > 2 or not positioned
    offset = 68 if extended else 44
    if len(data) != offset + frames * channels * 2 or data[:4] != b'RIFF' or data[8:16] != b'WAVEfmt ':
        raise ValueError('WAVE output geometry differs')
    if struct.unpack_from('<I', data, 4)[0] != len(data) - 8:
        raise ValueError('WAVE RIFF length differs')
    if struct.unpack_from('<HHI', data, 20) != (0xfffe if extended else 1, channels, 48000):
        raise ValueError('WAVE format/channels/rate differs')
    if extended and struct.unpack_from('<I', data, 40)[0] != (MASKS[channels - 1] if positioned else 0):
        raise ValueError('WAVE speaker mask differs')
    if data[offset - 8:offset - 4] != b'data' or struct.unpack_from('<I', data, offset - 4)[0] != len(data) - offset:
        raise ValueError('WAVE data length differs')
    return data[offset:]


def sources():
    files = [*ROOT.glob('src/**/*.rs'), ROOT/'Cargo.toml', ROOT/'Cargo.lock',
             ROOT/'rust-toolchain.toml', *[ROOT/'tools'/name for name in
             ['mapped_audio_interop.py', 'mapped_audio_fixtures.py', 'audio_baseline.py', 'multistream_decoder_fixtures.py']]]
    digest = hashlib.sha256()
    for path in sorted(files):
        digest.update(str(path.relative_to(ROOT)).encode() + b'\0' + path.read_bytes() + b'\0')
    return digest.hexdigest()


def run_cli(command):
    result = subprocess.run(command, capture_output=True)
    if result.returncode:
        raise ValueError(f'CLI failed: {result.stderr.decode(errors="replace")}')


def verify(lib, cli, directory):
    # Infer native WAVE map from its own surround factory plus the observed
    # FFmpeg permutation in captured fixtures; these are checked separately by
    # mapped_audio_fixtures.py and do not read the production layout module.
    wave_order = [[0], [0, 1], [0, 2, 1], [0, 1, 2, 3], [0, 2, 1, 3, 4],
                  [0, 2, 1, 5, 3, 4], [0, 2, 1, 6, 5, 3, 4], [0, 2, 1, 7, 5, 6, 3, 4]]
    rows = []
    for rate in [8000, 12000, 16000, 24000, 48000]:
        for channels in range(1, 9):
            for positioned in [True, False]:
                for duration, mode in [(n, m) for n in [120, 240, 480, 960, 1920, 2880, 3840, 4800, 5760] for m in ['cbr', 'vbr', 'cvbr']]:
                    ms = duration / 48
                    ms_text = str(ms).removesuffix('.0')
                    index = len(rows)
                    application = ['audio', 'voip', 'lowdelay', 'restricted-silk', 'restricted-celt'][index % 5]
                    if application == 'restricted-silk' and duration < 480:
                        application = 'restricted-celt'
                    frames = rate * duration // 48000 + 1
                    source, encoded, output = [directory / (str(index) + suffix) for suffix in ['.wav', '.opus', '-out.wav']]
                    source.write_bytes(wave(rate, channels, positioned, frames))
                    cap = (7656 if ms > 20 else 1276) if positioned and channels <= 2 else 7664 * channels
                    bitrate = 64000 * channels
                    command = [str(cli), 'encode', str(source), str(encoded), '--mode', 'auto',
                               '--application', application, '--complexity', '0', '--packet-ms', ms_text, '--frame-ms', ms_text,
                               '--bitrate', str(bitrate), '--max-packet-bytes', str(cap)]
                    if mode != 'cbr':
                        command.append('--' + mode)
                    run_cli(command)
                    file_packets, end = packets(encoded.read_bytes())
                    head, tags, *audio = file_packets
                    family = (0 if channels < 3 else 1) if positioned else 255
                    skip = 120 if application in ['lowdelay', 'restricted-celt'] else 312
                    if head[:10] != b'OpusHead' + bytes([1, channels]) or head[18] != family or struct.unpack_from('<H', head, 10)[0] != skip:
                        raise ValueError('Encoded header layout/skip differs')
                    if not tags.startswith(b'OpusTags') or end != frames * (48000 // rate) + skip:
                        raise ValueError('Header tags or final duration differs')
                    if family:
                        streams, coupled = head[19:21]
                        mapping = list(head[21:21 + channels])
                    else:
                        streams, coupled, mapping = 1, channels - 1, list(range(channels))
                    if len(mapping) != channels:
                        raise ValueError('Truncated channel mapping')
                    minimum = 2 * streams - 1 + (streams if ms == 100 else 0)
                    normalized_rate = min(max(bitrate, 500 * channels), 750000 * channels)
                    expected_cbr = min(max((normalized_rate * duration // 48000 + 4) // 8, minimum), cap)
                    for packet in audio:
                        if not minimum <= len(packet) <= cap or (mode == 'cbr' and len(packet) != expected_cbr):
                            raise ValueError('Whole-packet budget differs')
                    order = wave_order[channels - 1] if family == 1 else list(range(channels))
                    native = decode(lib, streams, coupled, [mapping[i] for i in order], audio, 0, None)
                    native = native[skip * channels:end * channels]
                    if len(native) != frames * (48000 // rate) * channels:
                        raise ValueError('Native trimmed duration differs')
                    expected = struct.pack('<' + 'h' * len(native), *native)
                    run_cli([str(cli), 'decode', str(encoded), str(output)])
                    actual = wave_pcm(output.read_bytes(), channels, positioned, frames * (48000 // rate))
                    if actual != expected:
                        raise ValueError(f'Native PCM differs in case{index}')
                    # A swapped input map can still decode consistently in two
                    # decoders: independently check source channel identity.
                    correlations = []
                    if rate == 48000 and frames >= 960 and application in ['audio', 'restricted-celt']:
                        for channel in range(channels):
                            signal = native[channel::channels]
                            energy = sum(a * a for a in signal)
                            scores = []
                            for candidate in range(channels):
                                target = [math.sin(2 * math.pi * (37 + 83 * candidate) * f / rate) for f in range(frames)]
                                norm = math.sqrt(energy * sum(b * b for b in target))
                                scores.append(sum(a * b for a, b in zip(signal, target)) / norm if norm else 0)
                            if scores[channel] < .8 or any(scores[channel] - value < .1 for j, value in enumerate(scores) if j != channel):
                                raise ValueError(f'Input channel identity differs in case{index}channel{channel}: {scores}')
                            correlations.append(scores[channel])
                    rows.append(dict(rate=rate, channels=channels, family=family, application=application,
                                     duration_ms=ms, mode=mode, frames=frames, packets=len(audio),
                                     pcm_sha256=hashlib.sha256(expected).hexdigest(), input_channel_correlations=correlations))
    if len(rows) != 2160:
        raise ValueError('Incomplete matrix')
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('cli', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('Report already exists')
    version = tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
    actual_version = subprocess.check_output([str(args.cli.resolve()), '--version'], text=True).strip()
    if actual_version != 'opura ' + version:
        raise ValueError('CLI version differs from source')
    before = sources()
    with tempfile.TemporaryDirectory(prefix='opura-mapped-file-interop-') as temporary:
        directory = Path(temporary)
        lib = compile_reference(args.source.resolve(), directory)
        setup(lib)
        rows = verify(lib, args.cli.resolve(), directory)
    if before != sources():
        raise ValueError('Source changed during validation')
    report = dict(schema=1, version=version, source_sha256=before,
                  cli_sha256=hashlib.sha256(args.cli.read_bytes()).hexdigest(),
                  git_revision=subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
                  worktree_modified=bool(subprocess.check_output(['git', '-C', str(ROOT), 'status', '--porcelain'], text=True).strip()),
                  reference_revision=REVISION, reference_cflags=CFLAGS,
                  generated_utc=datetime.now(timezone.utc).isoformat(), cases=rows,
                  scope='Generic mapped CLI file interoperability; no surround-analysis, quality or speed claim')
    with args.output.open('x') as output:
        json.dump(report, output, indent=2)
        output.write('\n')
    print(f'Native exact PCM and whole-packet budgets pass for{len(rows)}mapped CLI cases', flush=True)


if __name__ == '__main__':
    main()
