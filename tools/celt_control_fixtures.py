#!/usr/bin/env python3
"""Generate original CELT prefix, energy-positioned TF and spread evidence.

Optional pinned source oracle; prefix and spread decision blocks and tf_decode
are extracted unchanged. Synthetic prefix boundary packets use original entropy
encoding; ordinary test/build paths use only saved fixtures.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path);mode=parser.add_mutually_exclusive_group();mode.add_argument('--tables',action='store_true');mode.add_argument('--encode',action='store_true')
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
def declaration(text,signature):
    start=text.index(signature);return text[start:text.index(';',start)+1]
def function(text,signature):
    if text.count(signature)!=1:raise SystemExit('Unexpected original function signature')
    start=text.index(signature);brace=text.index('{',start);depth=1;end=brace+1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    return text[start:end]
def numbers(decl):
    body=decl[decl.index('{')+1:decl.rindex('}')];body=re.sub(r'/\*.*?\*/','',body,flags=re.S)
    return [int(v) for v in re.findall(r'-?\d+',body)]
header=(source/'celt/celt.h').read_text();decoder=(source/'celt/celt_decoder.c').read_text()
tf=declaration((source/'celt/celt.c').read_text(),'const signed char tf_select_table[4][8]')
tap=declaration(header,'static const unsigned char tapset_icdf[3]');spread=declaration(header,'static const unsigned char spread_icdf[4]')
if args.tables:
    values=numbers(tf)
    if len(values)!=32:raise SystemExit('Unexpected TF table size')
    print('// Generated from pinned Opus v1.6.1 by tools/celt_control_fixtures.py --tables.')
    print('// Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation; 2008 Gregory Maxwell.')
    print('// See THIRD_PARTY_NOTICES.md.')
    print('pub(super) const TF_SELECT: [[i8;8];4] = '+str([values[i:i+8] for i in range(0,32,8)])+';')
    print('pub(super) const TAPSET: [u8;3] = '+str(numbers(tap))+';')
    print('pub(super) const SPREAD: [u8;4] = '+str(numbers(spread))+';')
    raise SystemExit(0)
begin='   total_bits = len*8;';end='   intra_ener = tell+3<=total_bits ? ec_dec_bit_logp(dec, 3) : 0;'
if decoder.count(begin)!=1 or decoder.count(end)!=1:raise SystemExit('Unexpected prefix decision block')
prefix=decoder[decoder.index(begin):decoder.index(end)+len(end)]
begin='   tell = ec_tell(dec);\n   spread_decision = SPREAD_NORMAL;';end='      spread_decision = ec_dec_icdf(dec, spread_icdf, 5);'
if decoder.count(begin)!=1 or decoder.count(end)!=1:raise SystemExit('Unexpected spread decision block')
spread_block=decoder[decoder.index(begin):decoder.index(end)+len(end)]
wrapper='''
#include "celt.h"
#include "bands.h"
#include "modes.h"
#include "quant_bands.h"
#include "entdec.h"
#include "entenc.h"
#include "os_support.h"
'''+tf+'\n'+function(decoder,'static void tf_decode(')+r'''
void observe(ec_dec *dec,opus_int32 *out) {out[0]=dec->rng;out[1]=ec_tell(dec);out[2]=ec_tell_frac(dec);}
void controls(const unsigned char *data,int len,int LM,int start,int end,int C,int position,opus_int32 *out) {
    ec_dec inner,*dec=&inner;
    ec_dec_init(dec,(unsigned char*)data,len);
    if(position==1) for(int i=0;i<3;i++) ec_dec_bit_logp(dec,3);
    if(position==2) ec_dec_bits(dec,5);
    if(position==3) {ec_dec_uint(dec,17);ec_dec_bits(dec,3);}
    if(position>=4) {
        int target=len*8-(position==4?0:position==5?1:3);
        while(ec_tell(dec)<(unsigned)target) ec_dec_bits(dec,IMIN(25,target-ec_tell(dec)));
    }
    int M=1<<LM,total_bits,tell,silence,postfilter_pitch,postfilter_tapset,isTransient,shortBlocks,intra_ener;
    opus_val16 postfilter_gain;
'''+prefix+r'''
    out[0]=silence;out[1]=postfilter_pitch;out[2]=postfilter_gain;out[3]=postfilter_tapset;out[4]=isTransient;out[5]=shortBlocks;out[6]=intra_ener;
    observe(dec,out+7);
    CELTMode mode={0};mode.nbEBands=21;
    celt_glog energies[42]={0};
    unquant_coarse_energy(&mode,start,end,energies,intra_ener,dec,C,LM);
    observe(dec,out+10);
    for(int i=0;i<42;i++) out[13+i]=energies[i];
    int tf_res[21]={0};
    tf_decode(start,end,isTransient,tf_res,LM,dec);
    observe(dec,out+55);
    for(int i=0;i<21;i++) out[58+i]=tf_res[i];
    int spread_decision;
'''+spread_block+r'''
    out[79]=spread_decision;observe(dec,out+80);
}
void tf_only(const unsigned char *data,int len,int LM,int start,int end,int transient,int remaining,opus_int32 *out) {
    ec_dec dec;ec_dec_init(&dec,(unsigned char*)data,len);
    int target=len*8-remaining;
    while(ec_tell(&dec)<(unsigned)target) ec_dec_bits(&dec,IMIN(25,target-ec_tell(&dec)));
    int values[21]={0};tf_decode(start,end,transient,values,LM,&dec);
    observe(&dec,out);
    for(int i=0;i<21;i++) out[3+i]=values[i];
}
void encode_prefix(int LM,int octave,int pitch,int qg,int tap,int transient,int intra,unsigned char *data) {
    ec_enc enc;ec_enc_init(&enc,data,16);
    ec_enc_bit_logp(&enc,0,15);ec_enc_bit_logp(&enc,1,1);ec_enc_uint(&enc,octave,6);
    ec_enc_bits(&enc,pitch,4+octave);ec_enc_bits(&enc,qg,3);ec_enc_icdf(&enc,tap,tapset_icdf,2);
    if(LM) ec_enc_bit_logp(&enc,transient,3);
    ec_enc_bit_logp(&enc,intra,3);ec_enc_done(&enc);
    if(ec_get_error(&enc)) abort();
}
'''
encoder=(source/'celt/celt_encoder.c').read_text()
wrapper+=function(encoder,'static void tf_encode(')+r'''
int control_prefix(ec_enc *enc,ec_dec *dec,int position,int target,int pattern) {
    if(position==1){if(enc)ec_enc_uint(enc,13,17);else if(ec_dec_uint(dec,17)!=13)return -1;}
    if(position==2){for(int i=0;i<3;i++){if(enc)ec_enc_bit_logp(enc,i%2,2+i);else if(ec_dec_bit_logp(dec,2+i)!=i%2)return -1;}}
    unsigned random=0x54464354U+pattern;
    while((int)(enc?ec_tell(enc):ec_tell(dec))<target){
        unsigned width=IMIN(25,target-(int)(enc?ec_tell(enc):ec_tell(dec)));
        random=random*1664525U+1013904223U;
        unsigned value=(pattern==0?0:pattern==1?~0U:random)&((1U<<width)-1);
        if(enc)ec_enc_bits(enc,value,width);else if(ec_dec_bits(dec,width)!=value)return -1;
    }
    return 0;
}
int encode_tf_probe(int length,int LM,int start,int end,int transient,int position,int remaining,int pattern,int select,int requested,const int *decisions,int *out,unsigned char *data) {
    ec_enc enc;ec_enc_init(&enc,data,length);
    control_prefix(&enc,NULL,position,length*8-remaining,pattern);
    out[0]=enc.rng;out[1]=ec_tell(&enc);out[2]=ec_tell_frac(&enc);
    int values[21]={0};for(int i=start;i<end;i++)values[i]=decisions[i];
    tf_encode(start,end,transient,values,LM,select,&enc);
    out[3]=enc.rng;out[4]=ec_tell(&enc);out[5]=ec_tell_frac(&enc);
    for(int i=0;i<21;i++)out[6+i]=values[i];
    int spread_decision=SPREAD_NORMAL;
    if(ec_tell(&enc)+4<=length*8){spread_decision=requested;ec_enc_icdf(&enc,requested,spread_icdf,5);}
    out[27]=spread_decision;out[28]=enc.rng;out[29]=ec_tell(&enc);out[30]=ec_tell_frac(&enc);
    ec_enc_done(&enc);if(ec_get_error(&enc))return 0;
    ec_dec dec;ec_dec_init(&dec,data,length);
    if(control_prefix(NULL,&dec,position,length*8-remaining,pattern))return -1;
    if(dec.rng!=(unsigned)out[0]||ec_tell(&dec)!=(unsigned)out[1]||ec_tell_frac(&dec)!=(unsigned)out[2])return -2;
    int decoded[21]={0};tf_decode(start,end,transient,decoded,LM,&dec);
    for(int i=0;i<21;i++)if(decoded[i]!=values[i])return -3;
    if(dec.rng!=(unsigned)out[3]||ec_tell(&dec)!=(unsigned)out[4]||ec_tell_frac(&dec)!=(unsigned)out[5])return -4;
    int effective=SPREAD_NORMAL;if(ec_tell(&dec)+4<=length*8)effective=ec_dec_icdf(&dec,spread_icdf,5);
    if(effective!=spread_decision||dec.rng!=enc.rng||ec_tell(&dec)!=ec_tell(&enc)||ec_tell_frac(&dec)!=ec_tell_frac(&enc)||ec_get_error(&dec))return -5;
    return 1;
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-control-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),
                    *[str(source/'celt'/f) for f in ['quant_bands.c','laplace.c','mathops.c','entcode.c','entdec.c','entenc.c']],'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));u8=c.POINTER(c.c_ubyte);i32=c.POINTER(c.c_int32)
    lib.controls.argtypes=[u8,*[c.c_int]*6,i32];lib.controls.restype=None
    lib.encode_prefix.argtypes=[c.c_int]*7+[u8];lib.encode_prefix.restype=None
    lib.tf_only.argtypes=[u8,*[c.c_int]*6,i32];lib.tf_only.restype=None
    if args.encode:
        lib.encode_tf_probe.argtypes=[c.c_int]*10+[i32,i32,u8];lib.encode_tf_probe.restype=c.c_int
        print('# Original tf_encode and spreading field:',REVISION)
        print('# controls LM START END TRANSIENT POSITION REMAINING PATTERN SELECT REQUESTED HEX DECISIONS[21] BEFORE_RANGE BEFORE_TELL BEFORE_FRAC TF_RANGE TF_TELL TF_FRAC VALUES[21] SPREAD RANGE TELL FRAC')
        random=0x5446454e;rows=0;failed=0
        def next_random():
            global random
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
            return random
        for lm in range(4):
            for transient in range(2 if lm else 1):
                for start,end in [(0,21),(17,21),(5,13),(0,0),(21,21),(0,1),(20,21),(1,2)]:
                    for length in [2,3,8,32,1275]:
                        remaining_values=sorted(set([0,1,2,3,4,5,6,7,8,9,12,16,length*8-1])) if length<1275 else [length*8-1]
                        for remaining in remaining_values:
                            if remaining>=length*8:continue
                            for case in range(6):
                                for select in [0,1]:
                                    decisions=[int(case==1 or case==2 and i%2 or case==3 and i>=10 or case==4 and i==start or case==5 and next_random()&1) for i in range(21)]
                                    position=case%3;pattern=case%3;requested=case%4
                                    out=(c.c_int32*31)();data=(c.c_ubyte*length)()
                                    ok=lib.encode_tf_probe(length,lm,start,end,transient,position,remaining,pattern,select,requested,(c.c_int32*21)(*decisions),out,data)
                                    if ok<0:raise SystemExit(f'Original TF/spread roundtrip failure {lm,start,end,transient,length,remaining,case,select,ok}')
                                    if not ok:failed+=1;continue
                                    result=list(out)
                                    for i in [0,3,28]:result[i]=c.c_uint32(result[i]).value
                                    print('controls',lm,start,end,transient,position,remaining,pattern,select,requested,bytes(data).hex(),*decisions,*result);rows+=1
        print(f'# rows={rows} final-capacity-rejected={failed}')
        raise SystemExit
    print('# Original CELT prefix/coarse-positioned TF/spread:',REVISION)
    print('# frame LM START END CHANNELS POSITION HEX; prefix SILENCE PERIOD GAIN TAP TRANSIENT SHORT_BLOCKS INTRA RANGE TELL FRAC')
    print('# coarse RANGE TELL FRAC ENERGY_Q24[42]; tf RANGE TELL FRAC VALUES[21]; spread SYMBOL RANGE TELL FRAC')
    print('# tf_only LM START END TRANSIENT REMAINING HEX RANGE TELL FRAC VALUES[21]')
    random=0x434f4e54
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return random
    def emit(lm,start,end,channels,position,data,expected=None):
        out=(c.c_int32*83)();lib.controls((c.c_ubyte*len(data))(*data),len(data),lm,start,end,channels,position,out)
        if expected is not None and list(out[:7])!=expected:raise SystemExit('Synthetic control prefix mismatch')
        print('frame',lm,start,end,channels,position,data.hex())
        print('prefix',*out[:7],c.c_uint32(out[7]).value,*out[8:10])
        print('coarse',c.c_uint32(out[10]).value,*out[11:55])
        print('tf',c.c_uint32(out[55]).value,*out[56:79])
        print('spread',out[79],c.c_uint32(out[80]).value,*out[81:83])
    for lm in range(4):
        for start in [0,17]:
            for channels in [1,2]:
                for position in range(7):
                    for length in [2,3,4,8,16,40,1275]:
                        for pattern in range(3):
                            data=bytes([0]*length) if pattern==0 else bytes([255]*length) if pattern==1 else bytes(next_random()&255 for _ in range(length))
                            end=[13,17,19,21][(length+position)%4] if start==0 else 21
                            emit(lm,start,end,channels,position,data)
    for lm in range(4):
        for octave in range(6):
            for pitch in [0,(1<<(4+octave))-1]:
                for qg in [0,7]:
                    for tap in range(3):
                        for transient in range(2 if lm else 1):
                            for intra in range(2):
                                data=(c.c_ubyte*16)();lib.encode_prefix(lm,octave,pitch,qg,tap,transient,intra,data)
                                expected=[0,(16<<octave)+pitch-1,3072*(qg+1),tap,transient,(1<<lm) if transient else 0,intra]
                                emit(lm,0,21,2,0,bytes(data),expected)

    for lm in range(4):
        for transient in range(2):
            for start,end in [(0,21),(17,21),(5,13),(21,21)]:
                for remaining in [*range(17),32,120]:
                    for pattern in range(3):
                        data=bytes([0]*32) if pattern==0 else bytes([255]*32) if pattern==1 else bytes(next_random()&255 for _ in range(32))
                        out=(c.c_int32*24)();lib.tf_only((c.c_ubyte*32)(*data),32,lm,start,end,transient,remaining,out)
                        print('tf_only',lm,start,end,transient,remaining,data.hex(),c.c_uint32(out[0]).value,*out[1:])
