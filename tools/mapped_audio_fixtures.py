#!/usr/bin/env python3
"""Optional native Ogg/WAVE mapping oracle; never required by Cargo.

The unchanged pinned surround encoder supplies family1 maps/packets. WAVE
permutations are observed from FFmpeg decoding by channel correlation against
the fixed-point native decoder, rather than copied from the Rust layout table.
Only deterministic synthetic encoded files and PCM hashes are retained.
"""
import argparse
import ctypes as c
import math
from pathlib import Path
import struct
import subprocess
import tempfile
from audio_baseline import REVISION, compile_reference
from multistream_decoder_fixtures import checked, fingerprint


def crc(data):
    value = 0
    for byte in data:
        value ^= byte << 24
        for _ in range(8):
            value = ((value << 1) ^ (0x04c11db7 if value & 0x80000000 else 0)) & 0xffffffff
    return value


def ogg(head, packets, skip, trim):
    result = bytearray()
    tags = b'OpusTags' + struct.pack('<II', 0, 0)
    for sequence, packet in enumerate([head, tags, *packets]):
        last = sequence == len(packets) + 1
        granule = 0 if sequence < 2 else (sequence - 1) * 960 - (trim if last else 0)
        laces = bytes([255] * (len(packet) // 255) + [len(packet) % 255])
        if len(laces) > 255:
            raise ValueError('Fixture packet requires continuation')
        page = bytearray(b'OggS\0' + bytes([2 if sequence == 0 else 4 if last else 0]))
        page += struct.pack('<QIII', granule, 0x4f505552, sequence, 0)
        page += bytes([len(laces)]) + laces + packet
        page[22:26] = struct.pack('<I', crc(page))
        result += page
    if len(packets) * 960 - trim < skip:
        raise ValueError('Fixture has no duration after skip')
    return bytes(result)


def header(channels, streams, coupled, mapping, family, skip, gain):
    result = b'OpusHead' + struct.pack('<BBHIhB', 1, channels, skip, 48000, gain, family)
    return result + (bytes([streams, coupled, *mapping]) if family else b'')


def setup(lib):
    signatures = [
        ('encoder_create', [c.c_int]*4 + [c.c_void_p, c.c_int, c.c_void_p], c.c_void_p),
        ('surround_encoder_create', [c.c_int]*3 + [c.c_void_p]*3 + [c.c_int, c.c_void_p], c.c_void_p),
        ('decoder_create', [c.c_int]*4 + [c.c_void_p]*2, c.c_void_p),
        ('encode', [c.c_void_p, c.c_void_p, c.c_int, c.c_void_p, c.c_int], c.c_int),
        ('decode', [c.c_void_p, c.c_void_p, c.c_int, c.c_void_p, c.c_int, c.c_int], c.c_int),
    ]
    for name, args, result in signatures:
        function = getattr(lib, 'opus_multistream_' + name)
        function.argtypes, function.restype = args, result
    for kind in ['encoder', 'decoder']:
        getattr(lib, 'opus_multistream_' + kind + '_destroy').argtypes = [c.c_void_p]
        ctl = getattr(lib, 'opus_multistream_' + kind + '_ctl')
        ctl.argtypes, ctl.restype = [c.c_void_p, c.c_int], c.c_int


def encode(lib, channels, positioned):
    streams, coupled, error = c.c_int(), c.c_int(), c.c_int()
    mapping = (c.c_ubyte * channels)()
    if positioned:
        owner = lib.opus_multistream_surround_encoder_create(
            48000, channels, 0 if channels < 3 else 1,
            c.byref(streams), c.byref(coupled), mapping, 2049, c.byref(error))
    else:
        streams.value, coupled.value = channels, 0
        mapping[:] = range(channels)
        owner = lib.opus_multistream_encoder_create(
            48000, channels, streams, coupled, mapping, 2049, c.byref(error))
    checked(error.value)
    if not owner:
        raise ValueError('Native encoder creation failed')
    packets = []
    try:
        checked(lib.opus_multistream_encoder_ctl(owner, 4002, c.c_int(64000 * channels)))
        checked(lib.opus_multistream_encoder_ctl(owner, 4010, c.c_int(5)))
        seed = 4927
        for step in range(6):
            pcm = (c.c_int16 * (960 * channels))()
            for frame in range(960):
                for channel in range(channels):
                    seed = (1664525 * seed + 1013904223) & 0xffffffff
                    # Independent spectra distinguish channel permutations,
                    # including low-passed LFE, without saturating gain cases.
                    phase = 2 * math.pi * (37 + channel * 83) * (step * 960 + frame) / 48000
                    pcm[frame * channels + channel] = int(1800 * math.sin(phase)) + (seed >> 26) - 32
            output = (c.c_ubyte * 65536)()
            n = checked(lib.opus_multistream_encode(owner, pcm, 960, output, len(output)))
            if not n:
                raise ValueError('Empty native packet')
            packets.append(bytes(output[:n]))
    finally:
        lib.opus_multistream_encoder_destroy(owner)
    return streams.value, coupled.value, list(mapping), packets


def decode(lib, streams, coupled, mapping, packets, gain, expected_duration=960):
    error = c.c_int()
    channels = len(mapping)
    native_map = (c.c_ubyte * channels)(*mapping)
    owner = lib.opus_multistream_decoder_create(48000, channels, streams, coupled, native_map, c.byref(error))
    checked(error.value)
    if not owner:
        raise ValueError('Native decoder creation failed')
    result = []
    try:
        checked(lib.opus_multistream_decoder_ctl(owner, 4034, c.c_int(gain)))
        for packet in packets:
            data = (c.c_ubyte * len(packet)).from_buffer_copy(packet)
            pcm = (c.c_int16 * (5760 * channels))()
            n = checked(lib.opus_multistream_decode(owner, data, len(packet), pcm, 5760, 0))
            if n <= 0 or (expected_duration is not None and n != expected_duration):
                raise ValueError('Native packet duration differs')
            result.extend(pcm[:n * channels])
    finally:
        lib.opus_multistream_decoder_destroy(owner)
    return result


def observed_order(ffmpeg, path, native, channels, skip):
    result = subprocess.run([str(ffmpeg), '-v', 'error', '-i', str(path),
                             '-f', 's16le', '-acodec', 'pcm_s16le', '-'], check=True, capture_output=True)
    wave = struct.unpack('<' + 'h' * (len(result.stdout) // 2), result.stdout)
    expected = native[skip * channels:]
    if len(wave) != len(expected):
        raise ValueError('FFmpeg duration differs')
    order = []
    for output in range(channels):
        left = wave[output::channels]
        energy = sum(a * a for a in left)
        scores = []
        for source in range(channels):
            right = expected[source::channels]
            norm = math.sqrt(energy * sum(b * b for b in right))
            scores.append(sum(a * b for a, b in zip(left, right)) / norm if norm else 0)
        ranked = sorted(range(channels), key=lambda i: scores[i], reverse=True)
        if scores[ranked[0]] < .99 or (channels > 1 and scores[ranked[0]] - scores[ranked[1]] < .1):
            raise ValueError('Channel permutation is ambiguous')
        order.append(ranked[0])
    if sorted(order) != list(range(channels)):
        raise ValueError('FFmpeg order is not a channel permutation')
    return order


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--ffmpeg', type=Path, required=True)
    args = parser.parse_args()
    # New output only: failed partial runs cannot silently replace evidence.
    args.output.mkdir(parents=True, exist_ok=False)
    rows = []
    orders = {}
    three_channel_packets = None
    ffmpeg_version = subprocess.check_output([str(args.ffmpeg), '-version'], text=True).splitlines()[0]
    with tempfile.TemporaryDirectory(prefix='opura-mapped-audio-') as temporary:
        lib = compile_reference(args.source.resolve(), Path(temporary))
        setup(lib)
        for positioned in [True, False]:
            for channels in range(1, 9):
                streams, coupled, mapping, packets = encode(lib, channels, positioned)
                if positioned and channels == 3:
                    three_channel_packets = packets
                family = (0 if channels < 3 else 1) if positioned else 255
                baseline = decode(lib, streams, coupled, mapping, packets, 0)
                probe = Path(temporary) / 'probe.opus'
                probe.write_bytes(ogg(header(channels, streams, coupled, mapping, family, 312, 0), packets, 312, 0))
                order = observed_order(args.ffmpeg, probe, baseline, channels, 312) if positioned else list(range(channels))
                if positioned:
                    orders[channels] = order
                    print(f'channels{channels}: observed WAVE permutation {order}', flush=True)
                for gain, skip, trim in [(0, 312, 0), (1536, 120, 1), (-1536, 0, 959)]:
                    data = ogg(header(channels, streams, coupled, mapping, family, skip, gain), packets, skip, trim)
                    name = f'family{family}-{channels}ch-gain{gain}.opus'
                    (args.output / name).write_bytes(data)
                    pcm = decode(lib, streams, coupled, mapping, packets, gain)
                    frames = len(pcm) // channels
                    wave = [pcm[frame * channels + channel] for frame in range(skip, frames - trim) for channel in order]
                    rows.append(f'{name} {channels} {family} {frames - trim - skip} {fingerprint(wave)}')
        if three_channel_packets is None:
            raise ValueError('Missing native mapped packets')
        for family in [1, 255]:
            for label, mapping in [('repeat-silent', [2, 0, 1, 255, 0]), ('all-silent', [255]*4)]:
                channels = len(mapping)
                order = orders[channels] if family == 1 else list(range(channels))
                for gain in [0, 1536, -1536]:
                    skip, trim = 312, 1
                    name = f'family{family}-{label}-gain{gain}.opus'
                    (args.output / name).write_bytes(ogg(header(channels, 2, 1, mapping, family, skip, gain), three_channel_packets, skip, trim))
                    pcm = decode(lib, 2, 1, mapping, three_channel_packets, gain)
                    frames = len(pcm) // channels
                    wave = [pcm[frame * channels + channel] for frame in range(skip, frames - trim) for channel in order]
                    rows.append(f'{name} {channels} {family} {frames - trim - skip} {fingerprint(wave)}')
        (args.output / 'manifest.txt').write_text(
            '# Pinned original libopus ' + REVISION + '\n'
            '# ' + ffmpeg_version + '\n'
            + ''.join(f'# FFmpeg observed WAVE order{channels}: {order}\n' for channels, order in orders.items())
            + '# Synthetic native generic/surround packets; no perceptual quality claim\n'
            + '# FILE CHANNELS FAMILY FRAMES PCM16LE_FNV1A64\n' + '\n'.join(rows) + '\n')
    print(f'Captured {len(rows)} native mapped file cases', flush=True)


if __name__ == '__main__':
    main()
