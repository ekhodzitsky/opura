#!/usr/bin/env python3
"""Optional original budget admission/packet and retained-history oracle."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION
from opus_pcm_fixtures import pcm
from opus_long_input_fixtures import WRAPPER, CELT_WRAPPER
WRAPPER += r'''
int budget_probe(OpusEncoder *s,const opus_int16 *pcm,int samples,int mode,int wire,int band,int rate,int cap,unsigned char *packet,int *out,opus_uint64 *hashes) {
 if(opus_encoder_ctl(s,OPUS_SET_FORCE_MODE(mode)) || opus_encoder_ctl(s,OPUS_SET_FORCE_CHANNELS(wire)) ||
    opus_encoder_ctl(s,OPUS_SET_BANDWIDTH(band)) || opus_encoder_ctl(s,OPUS_SET_BITRATE(rate)))return -1;
 int size=opus_encoder_get_size(s->channels);unsigned char *saved=malloc(size);if(!saved)return -7;
 memcpy(saved,s,size);opus_uint64 old_mid=mid_hash;call_trace_reset();
 int result=opus_encode(s,pcm,samples,packet,cap);
 if(result<0){memcpy(s,saved,size);mid_hash=old_mid;call_trace_reset();}
 free(saved);
 int bytes=s->use_vbr?cap:IMAX(1,IMIN((bitrate_to_bits(s->bitrate_bps,s->Fs,samples)+4)/8,cap));
 out[0]=result>0&&(bytes<3||s->bitrate_bps<3*(s->Fs/samples)*8||((s->Fs/samples)<50&&(bytes*(s->Fs/samples)<300||s->bitrate_bps<2400)));
 out[1]=s->rangeFinal;out[2]=s->mode;out[3]=s->bandwidth;out[4]=s->stream_channels;out[5]=s->prev_mode;
 application_info(s,out+6,hashes);
 silk_encoder *speech=(silk_encoder*)((char*)s+s->silk_enc_offset);
 for(int ch=0;ch<2;ch++)hashes[ch+1]=s->application!=OPUS_APPLICATION_RESTRICTED_CELT&&ch<s->channels&&speech->state_Fxx[ch].sCmn.resampler_state.Fs_in_kHz?rs_hash(&speech->state_Fxx[ch].sCmn.resampler_state):0;
 hashes[3]=mid_hash;hashes[4]=s->width_mem.XX;hashes[5]=s->width_mem.XY;hashes[6]=s->width_mem.YY;hashes[7]=s->width_mem.smoothed_width;hashes[8]=s->width_mem.max_follower;
 return result;
}
'''
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path);parser.add_argument('output',type=Path)
    parser.add_argument('--calls-output',type=Path,required=True)
    args=parser.parse_args();source=args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise ValueError('Expected pinned Opus1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-long-input-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER)
        (folder/'transform.c').write_text(CELT_WRAPPER)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), str(folder/'transform.c'), *[str(source/f) for f in files if f not in ['src/opus_encoder.c','celt/celt_encoder.c']],
                        '-Wl,--wrap=silk_encode_do_VAD_FIX', '-Wl,--wrap=silk_InitEncoder', '-Wl,--wrap=celt_encode_with_ec',
                        '-lm', '-o', str(folder/'probe.so')], check=True)
        lib=c.CDLL(str(folder/'probe.so'))
        lib.application_new.argtypes=[c.c_int]*4;lib.application_new.restype=c.c_void_p
        lib.budget_probe.argtypes=[c.c_void_p,c.POINTER(c.c_int16),*[c.c_int]*6,c.POINTER(c.c_ubyte),c.POINTER(c.c_int),c.POINTER(c.c_uint64)]
        lib.budget_probe.restype=c.c_int
        lib.call_trace_get.argtypes=[c.POINTER(c.c_uint64)];lib.call_trace_get.restype=c.c_int
        lib.opus_encoder_destroy.argtypes=[c.c_void_p]
        rows=['# Original budget packets '+REVISION+'; negative calls restored transactionally',
              '# FS API APP VBR INDEX SAMPLES48 MODE REQUEST BAND RATE CAP SEED PATTERN RESULT EARLY RANGE LIVE_MODE LIVE_BAND LIVE_WIRE PREV_MODE HP2 HP1 LOOK DELAY_HASH RS0 RS1 MID_HASH WIDTH_XX WIDTH_XY WIDTH_YY WIDTH_SMOOTH WIDTH_FOLLOWER PACKET_HEX']
        calls=['# Original actual CELT calls: count samples/start/prediction/capacity/PCM_HASH']
        for rate in [8000,12000,16000,24000,48000]:
            for channels in [1,2]:
                for app in [2049,2048,2051,2052,2053]:
                    for vbr in [0,1]:
                        state=lib.application_new(rate,channels,app,vbr)
                        if not state:raise ValueError('Native init failed')
                        try:
                            for i in range(72):
                                n48=[120,240,480,960,1920,2880,3840,4800,5760][i%9]
                                if app==2052:n48=max(n48,480)
                                mode=[1000,1001,1002][i//4%3]
                                band=1103 if mode==1000 else 1105
                                wire=1+(channels-1)*(i//5%2)
                                bitrate=[100,500,1000,2000,64000,1500000][i//8%6]
                                cap=[1,2,3,7,11,128,640,32][i%8]
                                if i%12==11:cap=640;bitrate=64000*channels
                                seed=(0x62756467+i*982451653+rate*n48)&0xffffffff
                                pattern=0 if i%17==0 else 3
                                n=rate*n48//48000;data=(c.c_int16*(n*channels))(*pcm(seed,n*channels,pattern))
                                packet=(c.c_ubyte*7656)();out=(c.c_int*9)();hashes=(c.c_uint64*9)()
                                result=lib.budget_probe(state,data,n,mode,wire,band,bitrate,cap,packet,out,hashes)
                                rows.append(' '.join(map(str,[rate,channels,app,vbr,i,n48,mode,wire,band,bitrate,cap,seed,pattern,result,*out,*hashes,bytes(packet[:result]).hex() if result>0 else '-'])))
                                trace=(c.c_uint64*80)();count=lib.call_trace_get(trace)
                                calls.append(' '.join(map(str,[count//5,*trace[:count]])))
                        finally:lib.opus_encoder_destroy(state)
        args.output.write_text('\n'.join(rows)+'\n');args.calls_output.write_text('\n'.join(calls)+'\n')
        print('Captured',len(rows)-2,'original budget calls')
if __name__=='__main__':main()
