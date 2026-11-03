#!/usr/bin/env python3
"""Optional original public application input/history oracle, no Cargo dependency."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION
from opus_pcm_fixtures import pcm
from opus_stream_input_fixtures import WRAPPER, CELT_WRAPPER

# Restricted CELT has no allocated speech encoder: never inspect that offset.
WRAPPER = WRAPPER.replace(' silk_encoder *s=(silk_encoder*)((char*)st+st->silk_enc_offset);',
    ' if(st->application==OPUS_APPLICATION_RESTRICTED_CELT) {out[0]=1+((packet[0]>>2)&1);out[1]=out[2]=out[3]=0;hashes[0]=hashes[1]=0;hashes[2]=mid_hash;return length;}\n silk_encoder *s=(silk_encoder*)((char*)st+st->silk_enc_offset);')
WRAPPER += r'''
void highpass_probe(const opus_int16 *pcm, int *v, opus_int16 *out) {
 silk_encoder_state_FIX states[2];OPUS_CLEAR(states,2);
 silk_encoder_state *x=&states[0].sCmn;
 x->fs_kHz=v[3];x->prevLag=v[4];x->prevSignalType=v[5];
 x->input_quality_bands_Q15[0]=v[6];x->speech_activity_Q8=v[7];
 x->variable_HP_smth1_Q15=v[8];silk_HP_variable_cutoff(states);
 v[8]=x->variable_HP_smth1_Q15;
 v[9]=silk_SMLAWB(v[9],v[8]-v[9],SILK_FIX_CONST(VARIABLE_HP_SMTH_COEF2,16));
 v[14]=silk_log2lin(v[9]>>8);
 hp_cutoff(pcm,v[14],out,v+10,v[2],v[1],v[0],0);
}
void *application_new(int Fs,int channels,int application,int vbr) {
 int error;OpusEncoder *s=opus_encoder_create(Fs,channels,application,&error);
 if(!s||error)return NULL;
 if(opus_encoder_ctl(s,OPUS_SET_VBR(vbr)) || opus_encoder_ctl(s,OPUS_SET_VBR_CONSTRAINT(0)) ||
    opus_encoder_ctl(s,OPUS_SET_BITRATE(64000*channels)) || opus_encoder_ctl(s,OPUS_SET_COMPLEXITY(0)))return NULL;
 mid_target=&((silk_encoder*)((char*)s+s->silk_enc_offset))->state_Fxx[0];
 mid_hash=14695981039346656037ULL;
 return s;
}
void application_info(OpusEncoder *s,int *out,opus_uint64 *hashes) {
 silk_encoder *speech=(silk_encoder*)((char*)s+s->silk_enc_offset);
 out[0]=s->variable_HP_smth2_Q15;
 out[1]=s->application==OPUS_APPLICATION_RESTRICTED_CELT?0:speech->state_Fxx[0].sCmn.variable_HP_smth1_Q15;
 int lookahead;opus_encoder_ctl(s,OPUS_GET_LOOKAHEAD(&lookahead));out[2]=lookahead*48000/s->Fs;
 opus_uint64 h=14695981039346656037ULL;
 for(int i=0;i<s->encoder_buffer*s->channels;i++) {
  opus_uint16 v=(opus_uint16)s->delay_buffer[i];h^=v&255;h*=1099511628211ULL;h^=v>>8;h*=1099511628211ULL;
 }
 hashes[0]=h;
}
'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    parser.add_argument('--calls-output',type=Path)
    parser.add_argument('--hp-output',type=Path)
    args=parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')

    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-stream-input-') as temp:
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
        lib.application_new.argtypes=[c.c_int]*4
        lib.application_new.restype=c.c_void_p
        lib.stream_probe.argtypes=[c.c_void_p,c.POINTER(c.c_int16),*[c.c_int]*4,c.POINTER(c.c_int),c.POINTER(c.c_uint64)]
        lib.stream_probe.restype=c.c_int
        lib.application_info.argtypes=[c.c_void_p,c.POINTER(c.c_int),c.POINTER(c.c_uint64)]
        lib.opus_encoder_destroy.argtypes=[c.c_void_p]
        lib.call_trace_get.argtypes=[c.POINTER(c.c_uint64)]
        lib.call_trace_get.restype=c.c_int
        rows=['# Original applications '+REVISION+' scalar fixed fast64 Q24',
              '# RATE API APP VBR INDEX MS MODE REQUEST SEED PATTERN WIRE TO_MONO INTERNAL PREV_SPEECH RS0 RS1 MID_PCM_HASH HP2 HP1 LOOKAHEAD48 DELAY_HASH']
        calls=['# Per packet original CELT calls: count, samples48/start/prediction/capacity/PCM_FNV16']
        for rate in [8000,12000,16000,24000,48000]:
            for channels in [1,2]:
                for application in [2048,2051,2052,2053]:
                    for vbr in [0,1]:
                        state=lib.application_new(rate,channels,application,vbr)
                        if not state:raise ValueError('Native application init failed')
                        try:
                            for i in range(64):
                                ms=10 if i%2==0 else 20
                                mode=[1002,1000,1001,1002][i//8%4]
                                request=1+(channels-1)*(i//7%2)
                                seed=(0x61707073+i*982451653+rate*ms)&0xffffffff
                                pattern=0 if 16<=i<24 or 48<=i<56 else 1 if i%7==0 else 3
                                n=rate*ms//1000
                                values=(c.c_int16*(n*channels))(*pcm(seed,n*channels,pattern))
                                info=(c.c_int*4)();hashes=(c.c_uint64*3)()
                                length=lib.stream_probe(state,values,n,mode,request,-1,info,hashes)
                                if length<1:raise ValueError(f'Native failed {rate}/{channels}/{application}/{vbr}/{i}: {length}')
                                hp=(c.c_int*3)();delay=(c.c_uint64*1)()
                                lib.application_info(state,hp,delay)
                                rows.append(' '.join(map(str,[rate,channels,application,vbr,i,ms,mode,request,seed,pattern,*info,*hashes,*hp,*delay])))
                                trace=(c.c_uint64*40)();count=lib.call_trace_get(trace)
                                calls.append(' '.join(map(str,[count//5,*trace[:count]])))
                        finally:lib.opus_encoder_destroy(state)
        if args.hp_output:
            from opus_pcm_fixtures import fingerprint
            lib.highpass_probe.argtypes=[c.POINTER(c.c_int16),c.POINTER(c.c_int),c.POINTER(c.c_int16)]
            hp_rows=['# Original adaptive cutoff and hp_cutoff '+REVISION,
                     '# RATE API SAMPLES48 KHZ LAG SIGNAL QUALITY ACTIVITY HP1 HP2 SEED PATTERN BEFORE4 AFTER_HP1 AFTER_HP2 CUTOFF AFTER4 PCM_HASH']
            for rate in [8000,12000,16000,24000,48000]:
                for channels in [1,2]:
                    for samples48 in [120,240,480,960]:
                        for case in range(16):
                            khz=[8,12,16][case%3]
                            n=rate*samples48//48000
                            before=[rate,channels,n,khz,khz*(2+case),case%3,
                                    0 if case%4==0 else (case*2377)%32768,(case*37)%256,
                                    193536+case*384,193536+(15-case)*512]
                            delay=[case*1301-9000,case*71-500,7000-case*511,1300-case*53]
                            seed=(0x68706672+case*982451653+rate*samples48)&0xffffffff
                            pattern=case%4
                            data=(c.c_int16*(n*channels))(*pcm(seed,n*channels,pattern))
                            values=(c.c_int*15)(*before,*delay,0)
                            output=(c.c_int16*(n*channels))()
                            lib.highpass_probe(data,values,output)
                            hp_rows.append(' '.join(map(str,[rate,channels,samples48,*before[3:],seed,pattern,*delay,*values[8:10],values[14],*values[10:14],fingerprint(output)])))
            args.hp_output.write_text('\n'.join(hp_rows)+'\n')
            print('Captured',len(hp_rows)-2,'original adaptive highpass/filter cases')
        args.output.write_text('\n'.join(rows)+'\n')
        if args.calls_output:args.calls_output.write_text('\n'.join(calls)+'\n')
        print('Captured',len(rows)-2,'native application histories')


if __name__ == '__main__':
    main()
