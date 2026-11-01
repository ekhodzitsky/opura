#!/usr/bin/env python3
"""Generate classic CELT allocation tables/results from original pinned code.

Optional oracle: original dynamic allocation block/init_caps/rate.c plus
original entropy and fine energy. Position7 traverses original prefix/coarse/
TF/spread first; other positions target direct remaining-budget boundaries.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path);parser.add_argument('--tables',action='store_true')
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
def declaration(text,signature):
    start=text.index(signature);return text[start:text.index(';',start)+1]
def function(text,signature):
    if text.count(signature)!=1:raise SystemExit('Unexpected original function signature: '+signature)
    start=text.index(signature);brace=text.index('{',start);depth=1;end=brace+1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    return text[start:end]
def block(text,start,end,inclusive=True):
    if text.count(start)!=1 or text.count(end)!=1:raise SystemExit('Unexpected original block anchor')
    return text[text.index(start):text.index(end)+(len(end) if inclusive else 0)]
def numbers(text):
    body=text[text.index('{')+1:text.rindex('}')];body=re.sub(r'/\*.*?\*/','',body,flags=re.S)
    return [int(v) for v in re.findall(r'-?\d+',body)]
mode_source=(source/'celt/modes.c').read_text();static=(source/'celt/static_modes_fixed.h').read_text();rate=(source/'celt/rate.c').read_text();decoder=(source/'celt/celt_decoder.c').read_text();celt=(source/'celt/celt.c').read_text()
vectors=declaration(mode_source,'static const unsigned char band_allocation[]')
if args.tables:
    data=[('CAPS',168,'u8',numbers(declaration(static,'static const unsigned char cache_caps50[168]'))),
          ('LOG_N',21,'u8',numbers(declaration(static,'static const opus_int16 logN400[21]'))),
          ('LOG2_FRAC',24,'u8',numbers(declaration(rate,'static const unsigned char LOG2_FRAC_TABLE[24]'))),
          ('TRIM',11,'u8',numbers(declaration((source/'celt/celt.h').read_text(),'static const unsigned char trim_icdf[11]')))]
    print('// Generated from pinned Opus v1.6.1 by tools/celt_allocation_fixtures.py --tables.')
    print('// Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation; 2008 Gregory Maxwell.')
    print('// See THIRD_PARTY_NOTICES.md.')
    for name,length,kind,values in data:
        if len(values)!=length:raise SystemExit('Unexpected allocation table shape')
        print(f'pub(super) const {name}: [{kind};{length}] = '+str(values)+';')
    values=numbers(vectors)
    if len(values)!=231:raise SystemExit('Unexpected allocation vector count')
    print('pub(super) const VECTORS: [[u8;21];11] = '+str([values[i:i+21] for i in range(0,231,21)])+';')
    raise SystemExit(0)
prefix=block(decoder,'   total_bits = len*8;','   intra_ener = tell+3<=total_bits ? ec_dec_bit_logp(dec, 3) : 0;')
spread=block(decoder,'   tell = ec_tell(dec);\n   spread_decision = SPREAD_NORMAL;','      spread_decision = ec_dec_icdf(dec, spread_icdf, 5);')
allocation=block(decoder,'   ALLOC(cap, nbEBands, int);','   unquant_fine_energy(mode, start, end, oldBandE, NULL, fine_quant, dec, C);',False)
wrapper='''
#include "celt.h"
#include "bands.h"
#include "modes.h"
#include "quant_bands.h"
#include "rate.h"
#include "stack_alloc.h"
#include "os_support.h"
'''+declaration(mode_source,'static const opus_int16 eband5ms[]')+'\n'+vectors+'\n#include "static_modes_fixed.h"\n'+declaration(celt,'const signed char tf_select_table[4][8]')+'\n'+function(celt,'void init_caps(')+'\n'+function(decoder,'static void tf_decode(')+r'''
void observe(ec_dec *dec,opus_int32 *out) {out[0]=dec->rng;out[1]=ec_tell(dec);out[2]=ec_tell_frac(dec);}
void allocate(const unsigned char *data,int len,int LM,int C,int start,int end,int transient,int position,opus_int32 *out) {
    ec_dec inner,*dec=&inner;ec_dec_init(dec,(unsigned char*)data,len);
    const CELTMode *mode=&mode48000_960_120;
    const opus_int16 *eBands=mode->eBands;int nbEBands=21;
    celt_glog energy[42]={0};
    int total_bits=len*8,tell,isTransient=transient;
    if(position==1 || (position==7 && start==17)) for(int i=0;i<3;i++) ec_dec_bit_logp(dec,3);
    if(position==2) ec_dec_bits(dec,5);
    if(position==3) {ec_dec_uint(dec,17);ec_dec_bits(dec,3);}
    if(position>=4 && position<=6) {
        int target=len*8-(position==4?0:position==5?1:3);
        while(ec_tell(dec)<(unsigned)target) ec_dec_bits(dec,IMIN(25,target-ec_tell(dec)));
    }
    if(position==7) {
        int M=1<<LM,silence,postfilter_pitch,postfilter_tapset,shortBlocks,intra_ener;
        opus_val16 postfilter_gain;
'''+prefix+r'''
        unquant_coarse_energy(mode,start,end,energy,intra_ener,dec,C,LM);
        int tf_res[21]={0};tf_decode(start,end,isTransient,tf_res,LM,dec);
        int spread_decision;
'''+spread+r'''
    }
    total_bits=len*8;
    int i,dynalloc_logp,alloc_trim,bits,anti_collapse_rsv,codedBands,intensity=0,dual_stereo=0;
    opus_int32 balance=0;
    VARDECL(int,cap);VARDECL(int,offsets);VARDECL(int,fine_quant);VARDECL(int,pulses);VARDECL(int,fine_priority);
'''+allocation+r'''
    out[0]=isTransient;out[1]=alloc_trim;out[2]=anti_collapse_rsv;out[3]=codedBands;out[4]=intensity;out[5]=dual_stereo;out[6]=balance;
    observe(dec,out+7);
    for(int i=0;i<21;i++) {
        out[10+i]=cap[i];
        out[31+i]=(i>=start&&i<end)?offsets[i]:0;
        out[52+i]=(i>=start&&i<end)?pulses[i]:0;
        out[73+i]=(i>=start&&i<end)?fine_quant[i]:0;
        out[94+i]=(i>=start&&i<end)?fine_priority[i]:0;
    }
    unquant_fine_energy(mode,start,end,energy,NULL,fine_quant,dec,C);
    observe(dec,out+115);
    for(int i=0;i<42;i++) out[118+i]=energy[i];
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-allocation-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),
                    *[str(source/'celt'/f) for f in ['rate.c','quant_bands.c','laplace.c','mathops.c','entcode.c','entdec.c','entenc.c']],'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));u8=c.POINTER(c.c_ubyte);i32=c.POINTER(c.c_int32)
    lib.allocate.argtypes=[u8,*[c.c_int]*7,i32];lib.allocate.restype=None
    print('# Original dynamic allocation/init_caps/rate.c/fine energy:',REVISION)
    print('# frame LM START END CHANNELS TRANSIENT_ARG POSITION HEX')
    print('# allocation TRANSIENT TRIM ANTI_RSV CODED INTENSITY DUAL BALANCE RANGE TELL FRAC CAPS[21] OFFSETS[21] PVQ_BITS[21] FINE[21] PRIORITY[21]')
    print('# fine RANGE TELL FRAC ENERGY_Q24[42]')
    random=0x414c4c4f
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return random
    def emit(lm,channels,start,end,transient,position,data):
        length=len(data)
        out=(c.c_int32*160)();lib.allocate((c.c_ubyte*length)(*data),length,lm,channels,start,end,transient,position,out)
        print('frame',lm,start,end,channels,transient,position,data.hex())
        print('allocation',*out[:7],c.c_uint32(out[7]).value,*out[8:115])
        print('fine',c.c_uint32(out[115]).value,*out[116:])
    for lm in range(4):
        for channels in [1,2]:
            for start,end in [(0,21),(0,13),(0,19),(17,21),(0,1)]:
                for transient in range(2):
                    for position in range(8):
                        if position==7 and transient:continue
                        for length in [2,3,8,16,40,120,1275]:
                            for pattern in range(3):
                                data=bytes([0]*length) if pattern==0 else bytes([255]*length) if pattern==1 else bytes(next_random()&255 for _ in range(length))
                                emit(lm,channels,start,end,transient,position,data)

    # Exhaust every legal nonempty interval accepted by BandLayout.
    for lm in range(4):
        for channels in [1,2]:
            for start in range(21):
                for end in range(start+1,22):
                    for transient in range(2):
                        position=(lm+channels+start+end+transient)%7
                        data=bytes(next_random()&255 for _ in range(16))
                        emit(lm,channels,start,end,transient,position,data)
