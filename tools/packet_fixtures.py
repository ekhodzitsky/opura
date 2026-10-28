#!/usr/bin/env python3
"""Regenerate compact reference fixtures with an explicitly supplied libopus.

Optional developer tool, not used by cargo or CI. Only Python's standard
library is used. No foreign code is linked into opura.
Usage: python3 tools/packet_fixtures.py /path/to/libopus.so.0 > tests/packet-fixtures.txt
"""
import ctypes as c
import sys

lib = c.CDLL(sys.argv[1])
lib.opus_get_version_string.restype = c.c_char_p
lib.opus_packet_parse.argtypes = [c.POINTER(c.c_ubyte), c.c_int32,
    c.POINTER(c.c_ubyte), c.POINTER(c.POINTER(c.c_ubyte)),
    c.POINTER(c.c_int16), c.POINTER(c.c_int)]
lib.opus_packet_parse.restype = c.c_int
lib.opus_packet_get_samples_per_frame.argtypes = [c.POINTER(c.c_ubyte), c.c_int32]
lib.opus_packet_get_samples_per_frame.restype = c.c_int
lib.opus_packet_get_bandwidth.argtypes = [c.POINTER(c.c_ubyte)]
lib.opus_packet_get_bandwidth.restype = c.c_int
lib.opus_packet_get_nb_channels.argtypes = [c.POINTER(c.c_ubyte)]
lib.opus_packet_get_nb_channels.restype = c.c_int

print('# Reference:', lib.opus_get_version_string().decode())
print('# RFC 6716 section 3; framing only, payload bytes are synthetic.')
print('# Input hex (- = empty); result: error or samples/channel,channels,bandwidth,offset:length...')
# Short, inspectable cases spanning every TOC and each packing form.
cases = [b'']
for config in range(32):
    for stereo in range(2):
        toc = (config << 3) | (stereo << 2)
        cases.extend([bytes([toc, 42]), bytes([toc | 1, 11, 12]),
                      bytes([toc | 2, 0, 11]), bytes([toc | 3, 0x83, 0, 1, 11, 12, 13])])
for count in [0, 1, 2, 3, 6, 12, 24, 48, 49, 63]:
    cases.append(bytes([0x83, count]))
for data in [[1, 0], [2], [2, 252], [2, 1], [3], [3, 0x41], [3, 0x41, 255],
             [3, 0x41, 2, 0], [3, 0x82], [3, 0x82, 252], [3, 0x83, 2, 2, 0],
             [3, 0xc2, 1, 252, 0], [3, 0xc2, 2, 1, 11, 12, 99, 98],
             [3, 0x41, 0, 11], [3, 0x81, 11], [1], [2, 0]]:
    cases.append(bytes(data))
# Run-length syntax keeps large boundary cases compact and reviewable.
for size in [251, 252, 255, 256, 1275, 1276]:
    for header in [bytes([0]), bytes([1]), bytes([2, 0]), bytes([3, 1])]:
        cases.append(header + bytes([42]) * size)
for size in [252, 255, 256, 1275]:
    cases.append(bytes([2, 252 + size % 4, (size - 252) // 4]) + bytes([42]) * size)
cases.append(bytes([3, 0x41, 255, 0, 42]) + bytes([123]) * 254)
cases.append(bytes([0x83, 48]) + bytes(range(48)))
cases.append(bytes([0x83, 0x80 | 48]) + bytes([1]) * 47 + bytes(range(48)))
for padding in [False, True]:
    # Three VBR frames: 252, 255, and 1 bytes, with optional nonzero padding.
    header = bytes([3, 0x83 | (0x40 if padding else 0)])
    header += bytes([2]) if padding else b''
    cases.append(header + bytes([252, 0, 255, 0]) + bytes([42]) * 508
                 + (bytes([98, 99]) if padding else b''))


def encode(data):
    chunks = []
    i = 0
    while i < len(data):
        j = i + 1
        while j < len(data) and data[j] == data[i]:
            j += 1
        chunks.append(f'{data[i]:02x}*{j-i}' if j-i >= 4 else data[i:j].hex())
        i = j
    return ','.join(chunks) or '-'


for data in cases:
    array = (c.c_ubyte * max(1, len(data)))(*data)
    toc, offset = c.c_ubyte(), c.c_int()
    frames = (c.POINTER(c.c_ubyte) * 48)()
    sizes = (c.c_int16 * 48)()
    count = lib.opus_packet_parse(array, len(data), c.byref(toc), frames, sizes, c.byref(offset))
    if count < 0:
        result = 'error'
    else:
        samples = lib.opus_packet_get_samples_per_frame(array, 48000) * count
        channels = lib.opus_packet_get_nb_channels(array)
        bandwidth = lib.opus_packet_get_bandwidth(array)
        spans = [f'{c.cast(frames[i], c.c_void_p).value - c.addressof(array)}:{sizes[i]}' for i in range(count)]
        result = ','.join([str(samples), str(channels), str(bandwidth)] + spans)
    print(encode(data), result)
