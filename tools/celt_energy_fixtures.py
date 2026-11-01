#!/usr/bin/env python3
"""Generate classic CELT Q24 energy evidence from original pinned Opus routines.

Optional only; no reference code enters the Rust build. --tables emits the
coarse models. Default rows retain full energy arrays and entropy diagnostics
following each of the coarse/fine/final stages, across chained packet histories.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('--tables', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
    raise SystemExit('Reference checkout must be clean')
if args.tables:
    text = (source / 'celt/quant_bands.c').read_text()
    text = text[text.index('static const unsigned char e_prob_model'):text.index('static const unsigned char small_energy')]
    text = text[text.index('=')+1:text.index(';')]
    text = re.sub(r'/\*.*?\*/', '', text, flags=re.S)
    numbers = [int(v) for v in re.findall(r'\d+', text)]
    if len(numbers) != 336: raise SystemExit('Unexpected coarse model shape')
    pairs = [numbers[i:i+2] for i in range(0,336,2)]
    table = [[pairs[lm*42+intra*21:lm*42+(intra+1)*21] for intra in range(2)] for lm in range(4)]
    print('// Generated from Opus v1.6.1 by tools/celt_energy_fixtures.py --tables.')
    print('// Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation.')
    print('// See THIRD_PARTY_NOTICES.md.')
    print('pub(super) const PROBABILITY: [[[[u8;2];21];2];4] = ' + str(table) + ';')
    raise SystemExit(0)
wrapper = r'''
#include "quant_bands.h"
#include "laplace.h"
void observe_energy(ec_dec *dec, const celt_glog *energy, opus_int32 *out) {
    out[0]=dec->rng; out[1]=ec_tell(dec); out[2]=ec_tell_frac(dec);
    for (int i=0;i<42;i++) out[i+3]=energy[i];
}
int energy_probe(const unsigned char *data, int len, int lm, int channels, int start, int end, int intra,
                 celt_glog *energy, int *bits, int *priority, int requested, opus_int32 *out) {
    CELTMode mode = {0};
    mode.nbEBands=21;
    ec_dec dec;
    ec_dec_init(&dec,(unsigned char*)data,len);
    unquant_coarse_energy(&mode,start,end,energy,intra,&dec,channels,lm);
    observe_energy(&dec,energy,out);
    unquant_fine_energy(&mode,start,end,energy,NULL,bits,&dec,channels);
    observe_energy(&dec,energy,out+45);
    int remaining = len*8-ec_tell(&dec);
    int budget = IMIN(requested,IMAX(0,remaining));
    unquant_energy_finalise(&mode,start,end,energy,bits,priority,budget,&dec,channels);
    observe_energy(&dec,energy,out+90);
    return budget;
}
void laplace_probe(const unsigned char *data, int len, int fs, int decay, opus_int32 *out) {
    ec_dec dec;
    ec_dec_init(&dec,(unsigned char*)data,len);
    for(int i=0;i<16;i++) {
        out[4*i]=ec_laplace_decode(&dec,fs,decay);
        out[4*i+1]=dec.rng; out[4*i+2]=ec_tell(&dec); out[4*i+3]=ec_tell_frac(&dec);
    }
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-energy-') as temp:
    path = Path(temp)
    (path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),
                    *[str(source/'celt'/f) for f in ['quant_bands.c','laplace.c','entcode.c','entenc.c','entdec.c','mathops.c']],
                    '-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'))
    u8=c.POINTER(c.c_ubyte)
    i32=c.POINTER(c.c_int32)
    lib.energy_probe.argtypes=[u8,*[c.c_int]*6,i32,i32,i32,c.c_int,i32]
    lib.energy_probe.restype=c.c_int
    lib.laplace_probe.argtypes=[u8,c.c_int,c.c_int,c.c_int,i32]
    lib.laplace_probe.restype=None
    print('# Original fixed-point CELT energy/laplace:',REVISION)
    print('# frame RESET LM CHANNELS START END INTRA HEX BITS_HEX PRIORITY_BITS FINAL_BUDGET')
    print('# coarse/fine/final RANGE TELL TELL_FRAC Q24_ENERGIES[42]; laplace HEX FS0 DECAY VALUE/RANGE/TELL/FRAC[16]')
    random=0x454e4552
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff
        random ^= random>>17
        random ^= (random<<5)&0xffffffff
        return random
    for lm in range(4):
        for sequence in range(8):
            energies=(c.c_int32*42)()
            for index in range(24):
                length=[0,1,2,3,5,8,15,40,80,120,200][(index+sequence)%11]
                data=bytes(next_random()&255 for _ in range(length))
                if sequence==0:data=b'\0'*length
                if sequence==1:data=b'\xff'*length
                channels=1+(index//4+sequence)%2
                start=[0,0,17,5,21,1][(index+sequence)%6]
                end=max(start,[21,19,13,21][index%4])
                intra=int(index%5==0)
                bits=[(index+sequence+i)%9 for i in range(21)]
                if sequence==2:bits=[0]*21
                if sequence==3:bits=[8]*21
                priority=[(index+i)%2 for i in range(21)]
                buffer=(c.c_ubyte*length)(*data)
                out=(c.c_int32*135)()
                budget=lib.energy_probe(buffer,length,lm,channels,start,end,intra,energies,
                                        (c.c_int32*21)(*bits),(c.c_int32*21)(*priority),[0,1,3,7,42,1000][index%6],out)
                print('frame',int(index==0),lm,channels,start,end,intra,data.hex() if data else '-',
                      bytes(bits).hex(),''.join(map(str,priority)),budget)
                for offset,kind in [(0,'coarse'),(45,'fine'),(90,'final')]:
                    print(kind,c.c_uint32(out[offset]).value,*out[offset+1:offset+45])
    for fs in [1,3072,9216,16384,24576,32735,32736]:
        for decay in [0,1,64,1632,8192,11456]:
            for pattern in range(4):
                data=bytes([pattern*85]*32) if pattern<3 else bytes(next_random()&255 for _ in range(32))
                out=(c.c_int32*64)()
                lib.laplace_probe((c.c_ubyte*len(data))(*data),len(data),fs,decay,out)
                values=[]
                for i in range(16):values.extend([out[4*i],c.c_uint32(out[4*i+1]).value,out[4*i+2],out[4*i+3]])
                print('laplace',data.hex(),fs,decay,*values)
