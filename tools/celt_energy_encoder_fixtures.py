#!/usr/bin/env python3
"""Optional original CELT energy quantizers with paired original decoding.

Inactive native residual scratch is never observed before canonicalizing it to
zero; original two-pass code does not initialize those unused entries.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile

REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'


class Context(c.Structure):
    _fields_=[('buf',c.POINTER(c.c_ubyte)),('storage',c.c_uint32),('end_offs',c.c_uint32),
              ('end_window',c.c_uint32),('nend_bits',c.c_int),('nbits_total',c.c_int),
              ('offs',c.c_uint32),('rng',c.c_uint32),('val',c.c_uint32),('ext',c.c_uint32),
              ('rem',c.c_int),('error',c.c_int)]


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
    source=parser.parse_args().source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise SystemExit('Expected pinned Opus1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be clean')
    wrapper=r'''
#include "arch.h"
#include "modes.h"
#include "quant_bands.h"
#include "entenc.h"
#include "entdec.h"
#if !OPUS_FAST_INT64 || DB_SHIFT != 24
#error "Canonical scalar fixed-point fast64 Q24 required"
#endif
static const CELTMode mode={.nbEBands=21};
void coarse_probe(const int *p,const opus_int32 *target,opus_int32 *old,opus_int32 *error,opus_int32 *delayed,ec_enc *enc) {
    quant_coarse_energy(&mode,p[3],p[4],p[5],target,old,p[6]*8,error,enc,p[2],p[1],p[7],p[8],delayed,p[9],p[10],p[11]);
}
void fine_probe(const int *p,int *bits,opus_int32 *old,opus_int32 *error,ec_enc *enc) {
    quant_fine_energy(&mode,p[3],p[4],old,error,NULL,bits,enc,p[2]);
}
void final_probe(const int *p,int *bits,int *priority,int left,opus_int32 *old,opus_int32 *error,ec_enc *enc) {
    quant_energy_finalise(&mode,p[3],p[4],old,error,bits,priority,left,enc,p[2]);
}
void decode_coarse(const int *p,int intra,opus_int32 *old,ec_dec *dec) {
    unquant_coarse_energy(&mode,p[3],p[4],old,intra,dec,p[2],p[1]);
}
void decode_fine(const int *p,int *bits,opus_int32 *old,ec_dec *dec) {
    unquant_fine_energy(&mode,p[3],p[4],old,NULL,bits,dec,p[2]);
}
void decode_final(const int *p,int *bits,int *priority,int left,opus_int32 *old,ec_dec *dec) {
    unquant_energy_finalise(&mode,p[3],p[4],old,bits,priority,left,dec,p[2]);
}
'''
    with tempfile.TemporaryDirectory(prefix='opura-celt-energy-encoder-') as temp:
        path=Path(temp);(path/'probe.c').write_text(wrapper)
        subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                        '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),
                        *[str(source/'celt'/name) for name in ['quant_bands.c','mathops.c','laplace.c','entenc.c','entdec.c','entcode.c']],
                        '-o',str(path/'probe.so')],check=True)
        lib=c.CDLL(str(path/'probe.so'));i32=c.POINTER(c.c_int32);ctx=c.POINTER(Context);byte=c.POINTER(c.c_ubyte)
        signatures={
            'coarse_probe':([i32]*5+[ctx],None),'fine_probe':([i32]*4+[ctx],None),
            'final_probe':([i32]*3+[c.c_int,i32,i32,ctx],None),
            'decode_coarse':([i32,c.c_int,i32,ctx],None),'decode_fine':([i32,i32,i32,ctx],None),
            'decode_final':([i32,i32,i32,c.c_int,i32,ctx],None),
            'ec_enc_init':([ctx,byte,c.c_uint32],None),'ec_enc_uint':([ctx,c.c_uint32,c.c_uint32],None),
            'ec_enc_done':([ctx],None),'ec_dec_init':([ctx,byte,c.c_uint32],None),
            'ec_dec_uint':([ctx,c.c_uint32],c.c_uint32),'ec_dec_bit_logp':([ctx,c.c_uint],c.c_int),
            'ec_tell_frac':([ctx],c.c_uint32),
        }
        for name,(args,result) in signatures.items():getattr(lib,name).argtypes,getattr(lib,name).restype=args,result
        print('# Original coarse/fine/final energy quantizers and paired original decoder:',REVISION)
        print('# packet CONFIG[15] FINE[21] PRIORITY[21] TARGET_Q24[42] ENCODER_STAGES[3*(ENERGIES42 RESIDUALS42 RANGE TELL FRAC)] DELAYED INTRA FINAL_BUDGET UNUSED DECODER_STAGES[3*(ENERGIES42 RANGE TELL FRAC)] HEX')
        print('# CONFIG:RESET LM CHANNELS START END EFFECTIVE_END CAPACITY AVAILABLE_BYTES FORCE_INTRA TWO_PASS LOSS_PERCENT LFE PREFIX_BITS PREFIX_VALUE FINAL_REQUEST')
        print('# Unselected residual scratch is canonicalized to zero; histories persist. Explicit prefix-scarcity cases can reproduce native encoder/decoder history differences.')
        seed=0x454e4552
        def random_u32():
            nonlocal seed
            seed^=(seed<<13)&0xffffffff;seed^=seed>>17;seed^=(seed<<5)&0xffffffff
            return seed
        old=(c.c_int32*42)();decoded_old=(c.c_int32*42)();delayed=c.c_int32()
        def tell(context):return context.nbits_total-context.rng.bit_length()
        def snapshot(context,history,residuals=None):
            return [*history,*residuals] + [context.rng,tell(context),lib.ec_tell_frac(c.byref(context))] if residuals is not None else [*history,context.rng,tell(context),lib.ec_tell_frac(c.byref(context))]
        def emit(params,pattern):
            reset,lm,channels,start,end,effective_end,capacity,available,force,two,loss,lfe,prefix_bits,prefix_value,request=params
            if reset:
                for i in range(42):old[i]=decoded_old[i]=0
                delayed.value=0
            target=[]
            for channel in range(2):
                for band in range(21):
                    if channel>=channels or not start<=band<end:value=-2147483648
                    elif pattern==0:value=0
                    elif pattern==1:value=-(28<<24)
                    elif pattern==2:value=16<<24
                    elif pattern==3:value=(-20 if band%2 else 10)<<24
                    elif pattern==4:value=((-9+band%8)<<24)+(1<<23)-1
                    elif pattern==5:value=((-9+band%8)<<24)+(1<<23)
                    else:value=-(24<<24)+(random_u32()%(39<<24))
                    target.append(value)
            bits=[(band+pattern)%9 for band in range(21)]
            priorities=[(band+pattern+lm)%2 for band in range(21)]
            p=(c.c_int*15)(*params);q=(c.c_int*21)(*bits);prior=(c.c_int*21)(*priorities)
            values=(c.c_int32*42)(*target);errors=(c.c_int32*42)();data=(c.c_ubyte*capacity)();enc=Context()
            lib.ec_enc_init(c.byref(enc),data,capacity)
            if prefix_bits:lib.ec_enc_uint(c.byref(enc),prefix_value,1<<prefix_bits)
            lib.coarse_probe(p,values,old,errors,c.byref(delayed),c.byref(enc))
            for channel in range(2):
                for band in range(21):
                    if channel>=channels or not start<=band<end:errors[channel*21+band]=0
            stages=snapshot(enc,old,errors)
            lib.fine_probe(p,q,old,errors,c.byref(enc));stages+=snapshot(enc,old,errors)
            final_budget=min(request,max(0,capacity*8-tell(enc)));before=tell(enc)
            lib.final_probe(p,q,prior,final_budget,old,errors,c.byref(enc));stages+=snapshot(enc,old,errors)
            unused=final_budget-(tell(enc)-before)
            lib.ec_enc_done(c.byref(enc))
            if enc.error:raise RuntimeError('Original encoder overflow: '+str(params))
            dec=Context();lib.ec_dec_init(c.byref(dec),data,capacity)
            if prefix_bits:
                if lib.ec_dec_uint(c.byref(dec),1<<prefix_bits)!=prefix_value:raise RuntimeError('Original prefix mismatch')
            intra=lib.ec_dec_bit_logp(c.byref(dec),3) if tell(dec)+3<=capacity*8 else 0
            lib.decode_coarse(p,intra,decoded_old,c.byref(dec));decoder_stages=snapshot(dec,decoded_old)
            lib.decode_fine(p,q,decoded_old,c.byref(dec));decoder_stages+=snapshot(dec,decoded_old)
            lib.decode_final(p,q,prior,final_budget,decoded_old,c.byref(dec));decoder_stages+=snapshot(dec,decoded_old)
            if dec.error:raise RuntimeError('Original entropy decoder error')
            print('packet',*params,*bits,*priorities,*target,*stages,delayed.value,intra,final_budget,unused,*decoder_stages,bytes(data).hex())
        layouts=[(0,21),(0,13),(0,17),(17,21),(17,19),(5,13),(21,21)]
        for sequence in range(6):
            for index in range(64):
                lm=(index+sequence)%4;channels=1+(index//3+sequence)%2
                start,end=layouts[(index+sequence)%len(layouts)]
                capacity=[1,2,3,4,8,16,32,64,1275][(index+sequence)%9]
                prefixes=[b for b in [0,1,3,8,9,16,24] if b<capacity*8-3]
                prefix_bits=prefixes[(index//2+sequence)%len(prefixes)]
                prefix_value=random_u32()% (1<<prefix_bits)
                effective_end=max(start,end-(index%3))
                params=[int(index in [0,17,33]),lm,channels,start,end,effective_end,capacity,
                        [0,1,capacity//2,capacity][(index//4+sequence)%4],int(index%7==0),int(index%3!=0),
                        [0,1,10,30,100][(index//5+sequence)%5],int(index%11==0),prefix_bits,prefix_value,[0,1,2,7,42,10000][index%6]]
                emit(params,(index+sequence)%8)
        # First-band1-bit scarcity is intentionally retained, including native
        # reconstructed-history differences, rather than hidden by a round trip.
        for pattern in range(4):emit([1,0,1,0,13,13,2,2,0,0,0,0,14,0,0],pattern)


if __name__=='__main__':main()
