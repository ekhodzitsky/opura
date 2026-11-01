#!/usr/bin/env python3
"""Optional original Opus entropy encoder oracle; never used by Cargo builds.

Usage: python3 tools/range_encoder_fixtures.py /path/to/opus-v1.6.1
"""
import ctypes as c
from pathlib import Path
import random
import subprocess
import sys
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'


class Context(c.Structure):
    _fields_ = [('buf', c.POINTER(c.c_ubyte)), ('storage', c.c_uint32),
               ('end_offs', c.c_uint32), ('end_window', c.c_uint32),
               ('nend_bits', c.c_int), ('nbits_total', c.c_int),
               ('offs', c.c_uint32), ('rng', c.c_uint32), ('val', c.c_uint32),
               ('ext', c.c_uint32), ('rem', c.c_int), ('error', c.c_int)]


def main():
    if len(sys.argv) != 2:
        raise SystemExit('usage: range_encoder_fixtures.py OPUS_SOURCE')
    source = Path(sys.argv[1]).resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Pinned original Opus revision required')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
        raise SystemExit('Reference has modified tracked files')
    with tempfile.TemporaryDirectory(prefix='opura-range-encoder-') as temp:
        binary = Path(temp) / 'entropy.so'
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv',
                        '-I' + str(source / 'include'), '-I' + str(source / 'celt'),
                        *[str(source / 'celt' / name) for name in ['entenc.c', 'entdec.c', 'entcode.c', 'laplace.c']],
                        '-o', str(binary)], check=True)
        lib = c.CDLL(str(binary))
        ptr = c.POINTER(Context)
        signatures = {
            'ec_enc_init': ([ptr, c.POINTER(c.c_ubyte), c.c_uint32], None),
            'ec_encode': ([ptr, c.c_uint, c.c_uint, c.c_uint], None),
            'ec_enc_icdf': ([ptr, c.c_int, c.POINTER(c.c_ubyte), c.c_uint], None),
            'ec_enc_bit_logp': ([ptr, c.c_int, c.c_uint], None),
            'ec_enc_bits': ([ptr, c.c_uint32, c.c_uint], None),
            'ec_enc_uint': ([ptr, c.c_uint32, c.c_uint32], None),
            'ec_laplace_encode': ([ptr, c.POINTER(c.c_int), c.c_uint, c.c_int], None),
            'ec_enc_patch_initial_bits': ([ptr, c.c_uint, c.c_uint], None),
            'ec_enc_shrink': ([ptr, c.c_uint32], None),
            'ec_enc_done': ([ptr], None),
            'ec_tell_frac': ([ptr], c.c_uint32),
        }
        for name, (args, result) in signatures.items():
            getattr(lib, name).argtypes, getattr(lib, name).restype = args, result
        rng = random.Random(0x72616e67656e63)
        cases = [(1, []), (1, ['u:255:256']), (1, ['b:0:1','r:127:7']),
                 (2, ['r:255:8','r:255:8']), (1275, ['u:255:256'] * 1200),
                 (1275, ['u:255:256'] * 1200 + ['u:0:256']),
                 (64, ['r:33554431:25','r:33554431:25','s:8']),
                 (64, ['r:255:8','s:1']),
                 (8, ['u:0:2','p:1:1']),
                 (8, ['u:0:256','p:173:8']),
                 (8, ['u:0:256','u:0:256','u:0:256','p:173:8'])]
        for bits in range(1,9):
            for raw_bits in range(26):
                for value in [0,(1<<bits)-1]:
                    cases.append((max(1,(bits+raw_bits+7)//8),[f'u:{value}:{1<<bits}',f'r:{(1<<raw_bits)-1}:{raw_bits}']))
        for bits in range(1,9):
            for raw_bits in range(1,8):
                cases.append((max(1,(bits+raw_bits)//8),[f'u:0:{1<<bits}',f'r:{(1<<raw_bits)-1}:{raw_bits}']))
        for bits in range(1,8):
            cases.append((8,[f'u:0:{1<<bits}',f'u:0:{1<<(8-bits)}',f'p:{(1<<bits)-1}:{bits}']))
            cases.append((8,[f'u:0:{1<<bits}','u:0:256','u:0:256',f'p:{(1<<bits)-1}:{bits}']))
        # Deliberate termination overflow: the native sticky error becomes a
        # transactional Rust failure, compared against the state before finish.
        cases += [(1,['b:1:1','r:255:8']),(1,['r:511:9'])]
        for bits in range(1,9):
            cases.append((64, [f'u:0:{1<<bits}',f'p:{(1<<bits)-1}:{bits}', 'r:0:0','u:17:257','s:8']))
        for _ in range(100):
            ops = ['u:0:256']
            for step in range(60):
                kind = step % 6
                if kind == 0:
                    total = rng.choice([2,3,255,256,257,65535,65536,0xffffffff])
                    ops.append(f'u:{rng.randrange(total)}:{total}')
                elif kind == 1:
                    bits = rng.randrange(26)
                    ops.append(f'r:{rng.randrange(1<<bits)}:{bits}')
                elif kind == 2:
                    ops.append(f'b:{rng.randrange(2)}:{rng.randrange(1,16)}')
                elif kind == 3:
                    table = rng.choice([[0,1],[0,0,1,1,65535],[0,1,32768,65534,65535],[0,3,7,19]])
                    symbol = rng.choice([i for i in range(len(table)-1) if table[i] < table[i+1]])
                    ops.append('c:'+str(symbol)+':'+','.join(map(str,table)))
                elif kind == 4:
                    bits,table = rng.choice([(8,[255,254,128,1,0]),(3,[4,4,1,0]),(1,[0]),(8,[0])])
                    symbol = rng.choice([i for i in range(len(table)) if (table[i-1] if i else 1<<bits)>table[i]])
                    ops.append(f'i:{symbol}:{bits}:'+','.join(map(str,table)))
                else:
                    value = rng.choice([-2147483647,-20000,-17,-1,0,1,17,20000,2147483647])
                    ops.append(f'l:{value}:{rng.choice([1,32,6000,16000,32736])}:{rng.choice([0,1,6000,11456])}')
            ops += [f'p:{rng.randrange(256)}:8', 's:256']
            cases.append((1275,ops))
        print('# Original Opus1.6.1 encoder', REVISION)
        print('# case CAP; OP ACTUAL RANGE TELL FRAC FRONT BACK PENDING CARRY VALUE RAW RAW_BITS HEX')
        print('# ops u:value:total r:value:bits b:value:logp c:symbol:cdf i:symbol:bits:icdf l:value:zero:decay p:value:bits s:bytes init f')
        for capacity, ops in cases:
            buffer = (c.c_ubyte * capacity)()
            context = Context()
            lib.ec_enc_init(c.byref(context),buffer,capacity)
            print('case',capacity)
            for op in ['init',*ops,'f']:
                parts = op.split(':');kind = parts[0];args = parts[1:];actual = 0
                if kind in ['u','r','b','p']:
                    actual,extra = map(int,args)
                    if kind == 'u': lib.ec_enc_uint(c.byref(context),actual,extra)
                    elif kind == 'r' and extra: lib.ec_enc_bits(c.byref(context),actual,extra)
                    elif kind == 'b': lib.ec_enc_bit_logp(c.byref(context),actual,extra)
                    elif kind == 'p': lib.ec_enc_patch_initial_bits(c.byref(context),actual,extra)
                elif kind == 'c':
                    actual=int(args[0]);table=list(map(int,args[1].split(',')))
                    lib.ec_encode(c.byref(context),table[actual],table[actual+1],table[-1])
                elif kind == 'i':
                    actual,bits=map(int,args[:2]);table=list(map(int,args[2].split(',')))
                    lib.ec_enc_icdf(c.byref(context),actual,(c.c_ubyte*len(table))(*table),bits)
                elif kind == 'l':
                    val,zero,decay=map(int,args);v=c.c_int(val)
                    lib.ec_laplace_encode(c.byref(context),c.byref(v),zero,decay);actual=v.value
                elif kind == 's': lib.ec_enc_shrink(c.byref(context),int(args[0]))
                elif kind == 'f':
                    before,previous_bytes=bytes(context),bytes(buffer)
                    lib.ec_enc_done(c.byref(context))
                    if context.error:
                        c.memmove(c.byref(context),before,len(before))
                        c.memmove(buffer,previous_bytes,len(previous_bytes))
                        op='f!'

                if context.error: raise RuntimeError('Native fixture exceeds capacity: '+op)
                print(op,actual,context.rng,context.nbits_total-context.rng.bit_length(),lib.ec_tell_frac(c.byref(context)),
                      context.offs,context.end_offs,context.rem,context.ext,context.val,context.end_window,context.nend_bits,
                      bytes(buffer[:context.storage]).hex() if kind=='f' else '-')


if __name__ == '__main__':
    main()
