#!/usr/bin/env python3
"""Optional unchanged native SILK frame serializer and public decoder oracle.

The retained text fixtures need no compiler or native library during Cargo tests.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
WRAPPER = r'''
#include "main.h"
#include "opus.h"
#include <stdint.h>
int probe(int rate,int subframes,int frames,int raw,const int *parameters,
          const signed char *input,unsigned char *output,unsigned *info,uint64_t *hash) {
 silk_encoder_state s={0};
 ec_enc enc;
 int j,k,n=rate*subframes*5,length,error;
 unsigned char buffer[1275];
 s.fs_kHz=rate;s.nb_subfr=subframes;s.predictLPCOrder=rate==16?16:10;
 s.psNLSF_CB=rate==16?&silk_NLSF_CB_WB:&silk_NLSF_CB_NB_MB;
 s.pitch_lag_low_bits_iCDF=rate==8?silk_uniform4_iCDF:rate==12?silk_uniform6_iCDF:silk_uniform8_iCDF;
 s.pitch_contour_iCDF=subframes==2?(rate==8?silk_pitch_contour_10_ms_NB_iCDF:silk_pitch_contour_10_ms_iCDF)
                                    :(rate==8?silk_pitch_contour_NB_iCDF:silk_pitch_contour_iCDF);
 ec_enc_init(&enc,buffer,sizeof(buffer));
 if(!raw) {
  for(j=0;j<frames;j++)ec_enc_bit_logp(&enc,parameters[j*33]!=0,1);
  ec_enc_bit_logp(&enc,0,1);
 }
 for(j=0;j<frames;j++) {
  const int *p=parameters+j*33;
  signed char pulses[336]={0};
  s.indices.signalType=p[0];s.indices.quantOffsetType=p[1];
  for(k=0;k<4;k++)s.indices.GainsIndices[k]=p[2+k];
  for(k=0;k<17;k++)s.indices.NLSFIndices[k]=p[6+k];
  s.indices.NLSFInterpCoef_Q2=p[23];s.indices.lagIndex=p[24];
  s.indices.contourIndex=p[25];s.indices.PERIndex=p[26];
  for(k=0;k<4;k++)s.indices.LTPIndex[k]=p[27+k];
  s.indices.LTP_scaleIndex=p[31];s.indices.Seed=p[32];
  for(k=0;k<n;k++)pulses[k]=input[j*n+k];
  silk_encode_indices(&s,&enc,0,0,raw?CODE_INDEPENDENTLY_NO_LTP_SCALING:j?CODE_CONDITIONALLY:CODE_INDEPENDENTLY);
  silk_encode_pulses(&enc,p[0],p[1],pulses,n);
 }
 info[0]=enc.rng;info[1]=ec_tell(&enc);info[2]=ec_tell_frac(&enc);
 length=(ec_tell(&enc)+7)/8;
 ec_enc_shrink(&enc,length);ec_enc_done(&enc);
 if(enc.error)return -1;
 memcpy(output,buffer,length);info[3]=length;
 *hash=0;info[4]=0;info[5]=0;
 if(!raw) {
  unsigned char packet[1276];opus_int16 pcm[2880];
  int config=(rate==8?0:rate==12?4:8)+(subframes==2?0:frames);
  OpusDecoder *decoder=opus_decoder_create(48000,1,&error);
  if(!decoder || error)return -2;
  packet[0]=config<<3;memcpy(packet+1,buffer,length);
  k=opus_decode(decoder,packet,length+1,pcm,2880,0);
  if(k<0){opus_decoder_destroy(decoder);return k;}
  info[4]=k;
  opus_decoder_ctl(decoder,OPUS_GET_FINAL_RANGE(&info[5]));
  *hash=UINT64_C(14695981039346656037);
  for(j=0;j<k;j++) {
   unsigned value=(uint16_t)pcm[j];
   *hash=(*hash^(value&255))*UINT64_C(1099511628211);
   *hash=(*hash^(value>>8))*UINT64_C(1099511628211);
  }
  opus_decoder_destroy(decoder);
 }
 return 0;
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be clean')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise SystemExit('Missing source manifest')
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    rows = ['# Original encode_indices/encode_pulses and public fixed-point opus_decode: '+REVISION,
            '# KIND(0=packet,1=unscaled,2=native-capacity-error) RATE SUBFRAMES FRAMES RANGE TELL FRAC BYTES PCM_COUNT PCM_RANGE PCM_FNV64 PARAMS[33*FRAMES] PULSES[n*FRAMES] HEX']
    state = 0x53494c4b
    def random(n):
        nonlocal state
        state = (1664525*state+1013904223)&0xffffffff
        return state % n
    with tempfile.TemporaryDirectory(prefix='opura-silk-encoder-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files], '-lm', '-o', str(folder/'probe.so')], check=True)
        lib = c.CDLL(str(folder/'probe.so'))
        lib.probe.argtypes = [c.c_int]*4+[c.POINTER(c.c_int),c.POINTER(c.c_int8),c.POINTER(c.c_ubyte),c.POINTER(c.c_uint),c.POINTER(c.c_uint64)]
        lib.probe.restype = c.c_int
        for raw in [0, 1]:
            for rate in [8, 12, 16]:
                for subframes in [2, 4]:
                    for variant in range(3 if raw else 12):
                        frames = 1 if raw or subframes == 2 else variant%3+1
                        n = rate*subframes*5
                        parameters, pulses = [], []
                        previous_lag = 0
                        for frame in range(frames):
                            patterns = [[2,2,2]]*4+[[2,1,2],[2,0,2],[0,0,0],[1,1,1],[0,1,2],[2,2,0],[1,2,2],[0,2,2]]
                            signal = variant if raw else patterns[variant][frame]
                            offset = random(2)
                            gains = [random(41 if frame else 64)]+[random(41) for _ in range(3)]
                            nlsf = [random(32)]+[random(21)-10 for _ in range(16)]
                            interpolation = 4 if subframes == 2 else random(5)
                            lag = random(16*rate) if not frame else max(0,min(16*rate-1,previous_lag+[-9,-8,0,11,12][(variant+frame)%5]))
                            previous_lag = lag
                            contour_count = (3 if rate == 8 else 12) if subframes == 2 else (11 if rate == 8 else 34)
                            periodicity = (variant+frame)%3
                            scale = random(3) if not raw and frame == 0 else 0
                            parameters.extend([signal,offset,*gains,*nlsf,interpolation,lag,random(contour_count),periodicity,
                                               *[random(8<<periodicity) for _ in range(4)],scale,random(4)])
                            for i in range(n):
                                if variant%4 == 0: pulse = 0
                                elif variant%4 == 1: pulse = (random(3)-1) if i%7 == 0 else 0
                                elif variant%4 == 2: pulse = random(255)-127
                                else: pulse = (-128 if i%2 else 127)
                                pulses.append(pulse)
                        params = (c.c_int*len(parameters))(*parameters)
                        pulse_input = (c.c_int8*len(pulses))(*pulses)
                        output, info, pcm_hash = (c.c_ubyte*1275)(), (c.c_uint*6)(), c.c_uint64()
                        result = lib.probe(rate, subframes, frames, raw, params, pulse_input, output, info, c.byref(pcm_hash))
                        capacity_error = result == -1 and info[3] == 0 and info[1] > 1275*8
                        if result and not capacity_error:
                            raise SystemExit(f'Native encoding/decoding failed: {result}; raw={raw}, rate={rate}, subframes={subframes}, variant={variant}, info={list(info)}')
                        kind = 2 if capacity_error else raw
                        encoded = '-' if capacity_error else bytes(output[:info[3]]).hex()
                        rows.append(' '.join(map(str,[kind,rate,subframes,frames,*info,pcm_hash.value,*parameters,*pulses]))+' '+encoded)
    args.output.write_text('\n'.join(rows)+'\n')
    print(f'Wrote {len(rows)-2} native frame sequences')


if __name__ == '__main__':
    main()
