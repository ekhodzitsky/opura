#!/usr/bin/env python3
"""Generate entropy fixtures from an external, pinned Opus source checkout.

Requires Python, Git and a C compiler only for regeneration, never for Cargo.
Usage: python3 tools/range_fixtures.py /path/to/opus-v1.6.1 > /tmp/range-fixtures.txt
The temporary reference shared library is deleted on exit.
"""
import ctypes as c
from pathlib import Path
import subprocess
import sys
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
source = Path(sys.argv[1]).resolve()
revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
if revision != REVISION:
    raise SystemExit(f'Expected Opus v1.6.1 at {REVISION}, got {revision}')
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')


class Context(c.Structure):
    _fields_ = [('buf', c.POINTER(c.c_ubyte)), ('storage', c.c_uint32),
               ('end_offs', c.c_uint32), ('end_window', c.c_uint32),
               ('nend_bits', c.c_int), ('nbits_total', c.c_int),
               ('offs', c.c_uint32), ('rng', c.c_uint32), ('val', c.c_uint32),
               ('ext', c.c_uint32), ('rem', c.c_int), ('error', c.c_int)]


seed = 0x72616e67


def random_u32():
    global seed
    seed ^= (seed << 13) & 0xffffffff
    seed ^= seed >> 17
    seed ^= (seed << 5) & 0xffffffff
    return seed


with tempfile.TemporaryDirectory(prefix='opura-range-reference-') as temp:
    library = str(Path(temp) / 'entropy.so')
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-I' + str(source / 'include'),
                    '-I' + str(source / 'celt'), str(source / 'celt/entdec.c'),
                    str(source / 'celt/entcode.c'), '-o', library], check=True)
    lib = c.CDLL(library)
    ptr = c.POINTER(Context)
    signatures = {
        'ec_dec_init': ([ptr, c.POINTER(c.c_ubyte), c.c_uint32], None),
        'ec_decode': ([ptr, c.c_uint], c.c_uint),
        'ec_dec_update': ([ptr, c.c_uint, c.c_uint, c.c_uint], None),
        'ec_dec_icdf': ([ptr, c.POINTER(c.c_ubyte), c.c_uint], c.c_int),
        'ec_dec_bit_logp': ([ptr, c.c_uint], c.c_int),
        'ec_dec_bits': ([ptr, c.c_uint], c.c_uint32),
        'ec_dec_uint': ([ptr, c.c_uint32], c.c_uint32),
        'ec_tell_frac': ([ptr], c.c_uint32),
    }
    for name, (arguments, result) in signatures.items():
        fn = getattr(lib, name)
        fn.argtypes, fn.restype = arguments, result

    print('# Opus v1.6.1 entropy reference:', REVISION)
    print('# frame HEX (- = empty); OP RESULT RANGE TELL TELL_FRAC')
    print('# b:logp r:bits u:total c:boundaries i:precision:inverse_cdf')
    print('# corrupt means ec_dec_uint set its error flag; restore state for the Rust error contract.')
    inputs = [b'', b'\x00', b'\xff', b'\x81', bytes(32), bytes([255]) * 32, bytes(range(32))]
    inputs += [bytes(random_u32() & 255 for _ in range(64)) for _ in range(12)]
    for data in inputs:
        array = (c.c_ubyte * max(1, len(data)))(*data)
        context = Context()
        lib.ec_dec_init(c.byref(context), array, len(data))
        raw_used = 0
        print('frame', data.hex() or '-')
        print('init', 0, context.rng, context.nbits_total - context.rng.bit_length(), lib.ec_tell_frac(c.byref(context)))
        for step in range(80):
            backup = bytes(context)
            raw_bits = 0
            kind = step % 5
            if kind == 0:
                precision = 1 + random_u32() % 15
                operation = f'b:{precision}'
                value = lib.ec_dec_bit_logp(c.byref(context), precision)
            elif kind == 1:
                table = [[0, 1], [0, 1, 3, 7], [0, 0, 1, 1, 65535],
                         [0, 1, 32768, 65534, 65535]][random_u32() % 4]
                operation = 'c:' + ','.join(map(str, table))
                frequency = lib.ec_decode(c.byref(context), table[-1])
                value = next(i for i in range(len(table)-1) if frequency < table[i+1])
                lib.ec_dec_update(c.byref(context), table[value], table[value+1], table[-1])
            elif kind == 2:
                precision, table = [(8, [255, 254, 128, 1, 0]), (3, [4, 4, 1, 0]),
                                    (1, [0]), (8, [0])][random_u32() % 4]
                operation = f'i:{precision}:' + ','.join(map(str, table))
                value = lib.ec_dec_icdf(c.byref(context), (c.c_ubyte * len(table))(*table), precision)
            elif kind == 3:
                raw_bits = min(random_u32() % 26, len(data) * 8 - raw_used)
                operation = f'r:{raw_bits}'
                value = lib.ec_dec_bits(c.byref(context), raw_bits)
            else:
                total = [2, 3, 255, 256, 257, 65535, 65536, 0xffffffff][random_u32() % 8]
                raw_bits = max(0, (total - 1).bit_length() - 8)
                if raw_bits > len(data) * 8 - raw_used:
                    total, raw_bits = 256, 0
                operation = f'u:{total}'
                value = lib.ec_dec_uint(c.byref(context), total)
            if context.error:
                value = 'corrupt'
                c.memmove(c.byref(context), backup, len(backup))
            else:
                raw_used += raw_bits
            print(operation, value, context.rng, context.nbits_total - context.rng.bit_length(),
                  lib.ec_tell_frac(c.byref(context)))
