#!/usr/bin/env python3
"""Generate scalar fixed-point CELT math evidence from clean pinned Opus.

Optional oracle only: original mathops.c/mathops.h are compiled externally.
Block rows hash every signed result as four little-endian bytes using FNV-1a;
case rows store wide/normalization boundary values and fractional division.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
    raise SystemExit('Reference checkout must be clean')
wrapper = r'''
#include "mathops.h"
#if !OPUS_FAST_INT64
#error "Canonical fixture regeneration requires the scalar fast-64-bit reference path"
#endif
int probe(int operation, int x, int y) {
    switch (operation) {
    case 0: return x == 0 ? 0 : isqrt32((opus_uint32)x);
    case 1: return celt_sqrt(x);
    case 2: return celt_rsqrt_norm(x);
    case 3: return celt_cos_norm(x);
    case 4: return celt_log2(x);
    case 5: return celt_exp2((opus_int16)x);
    case 6: return celt_rcp(x);
    case 7: return frac_div32_q29(x,y);
    case 8: return frac_div32(x,y);
    default: abort();
    }
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-math-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DFIXED_POINT',
                    '-DDISABLE_FLOAT_API', '-DOPUS_BUILD',
                    '-I' + str(source / 'include'), '-I' + str(source / 'celt'),
                    str(path / 'probe.c'), str(source / 'celt/mathops.c'),
                    str(source / 'celt/entcode.c'), '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.c_int, c.c_int, c.c_int]
    lib.probe.restype = c.c_int
    print('# Original fixed-point CELT mathops.c/mathops.h:', REVISION)
    print('# block OP START COUNT FNV1A64(le-i32-results); case OP X Y RESULT')
    print('# OP 0=isqrt32 1=sqrt 2=rsqrt_norm 3=cos_norm 4=log2 5=exp2 6=rcp 7=frac_div_q29 8=frac_div_q31')
    for operation, start, stop in [(0,0,65536), (1,0,65536), (2,16384,65536),
                                   (3,-131072,131072), (4,0,65536),
                                   (5,-32768,32768), (6,1,65536)]:
        for first in range(start, stop, 1024):
            count = min(1024, stop-first)
            fingerprint = 14695981039346656037
            for value in range(first, first+count):
                result = lib.probe(operation,value,0)
                for byte in (result & 0xffffffff).to_bytes(4,'little'):
                    fingerprint = ((fingerprint ^ byte) * 1099511628211) & 0xffffffffffffffff
            print('block', operation, first, count, fingerprint)
    values = {0, 1, 0x7fffffff, 0xffffffff}
    for bit in range(32):
        values.update(v for v in range((1<<bit)-3,(1<<bit)+4) if 0 <= v <= 0xffffffff)
    random = 0x43454c54
    for _ in range(2048):
        random ^= (random << 13) & 0xffffffff
        random ^= random >> 17
        random ^= (random << 5) & 0xffffffff
        values.add(random)
    for value in sorted(values):
        print('case',0,value,0,lib.probe(0,c.c_int(value).value,0))
        if value <= 0x7fffffff:
            for operation in [1,3,4,6] if value else [1,3,4]:
                print('case',operation,value,0,lib.probe(operation,value,0))
    denominators = {1,2,3,32767,32768,32769,65535,65536,0x7fffffff}
    denominators.update(v for v in values if 0 < v <= 0x7fffffff)
    for denominator in sorted(denominators):
        for numerator in sorted({-denominator,-denominator+1,-denominator//2,-1,0,1,denominator//2,denominator-1,denominator}):
            for operation in [7,8]:
                print('case',operation,numerator,denominator,lib.probe(operation,numerator,denominator))
