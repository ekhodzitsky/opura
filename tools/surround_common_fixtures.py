#!/usr/bin/env python3
"""Optional actual original common-owner mask/LFE target, fade and call oracle."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import CFLAGS, REVISION, run
from opus_application_fixtures import WRAPPER
from opus_stream_input_fixtures import CELT_WRAPPER
from surround_analysis_fixtures import signal, fingerprint
from surround_celt_fixtures import masks

EXTRA = r'''
int surround_common_controls(OpusEncoder *st,int bitrate,int variable,int constrained,int lfe,celt_glog *mask) {
 int r=0;
 r|=opus_encoder_ctl(st,OPUS_SET_BITRATE(bitrate));
 r|=opus_encoder_ctl(st,OPUS_SET_VBR(variable));
 r|=opus_encoder_ctl(st,OPUS_SET_VBR_CONSTRAINT(constrained));
 r|=opus_encoder_ctl(st,OPUS_SET_LFE(lfe));
 r|=opus_encoder_ctl(st,OPUS_SET_ENERGY_MASK(mask));
 return r;
}
void surround_common_info(OpusEncoder *st,int *out) {
 out[0]=st->silk_mode.bitRate;out[1]=st->prev_HB_gain;
 out[2]=st->hybrid_stereo_width_Q14;out[3]=st->mode;out[4]=st->bandwidth;
}
'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    parser.add_argument('--long',action='store_true',help='actual10..120ms shared-context and transition scheduling')
    args=parser.parse_args()
    source=args.source.resolve()
    if args.output.exists():raise ValueError('Output must be new')
    if run(['git','-C',source,'rev-parse','HEAD']).strip()!=REVISION or run(['git','-C',source,'status','--porcelain']).strip():
        raise ValueError('Expected unchanged pinned source')
    def manifest(file,name):
        match=re.search(r'^'+name+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
        if match is None:raise ValueError('Missing source manifest')
        return match[1].replace('\\','').split()
    files=manifest('silk_sources.mk','SILK_SOURCES')+manifest('silk_sources.mk','SILK_SOURCES_FIXED')+manifest('celt_sources.mk','CELT_SOURCES')+manifest('opus_sources.mk','OPUS_SOURCES')
    rows=0
    with tempfile.TemporaryDirectory(prefix='opura-surround-common-') as temporary:
        directory=Path(temporary)
        original=(source/'celt/celt_encoder.c').read_text()
        if original.count('opus_val16 x1, x2;')!=1:raise ValueError('Q24 spectral patch target changed')
        (directory/'corrected-celt.c').write_text(original.replace('opus_val16 x1, x2;','celt_glog x1, x2;'))
        transform=CELT_WRAPPER.replace('#include "celt_encoder.c"','#include "corrected-celt.c"').replace('calls[40]','calls[224]').replace('call_count+5>40','call_count+7>224')
        needle='calls[call_count++]=cap;calls[call_count++]=h;'
        assert transform.count(needle)==1
        transform=transform.replace(needle,needle+r'''
 calls[call_count++]=st->lfe;
 opus_uint64 mask_hash=st->energy_mask?14695981039346656037ULL:0;
 if(st->energy_mask)for(int i=0;i<st->channels*21;i++)for(int b=0;b<4;b++) {
  mask_hash^=((opus_uint32)st->energy_mask[i]>>(8*b))&255;mask_hash*=1099511628211ULL;
 }
 calls[call_count++]=mask_hash;
''')
        (directory/'common.c').write_text(WRAPPER+EXTRA)
        (directory/'transform.c').write_text(transform)
        includes=['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']]
        library=directory/'oracle.so'
        subprocess.run(['cc',*CFLAGS,*includes,str(directory/'common.c'),str(directory/'transform.c'),
                        *[str(source/f) for f in files if f not in ['src/opus_encoder.c','celt/celt_encoder.c']],
                        '-Wl,--wrap=silk_encode_do_VAD_FIX','-Wl,--wrap=silk_InitEncoder','-Wl,--wrap=celt_encode_with_ec','-lm','-o',str(library)],check=True)
        lib=c.CDLL(str(library))
        signatures=[('application_new',[c.c_int]*4,c.c_void_p),
                    ('surround_common_controls',[c.c_void_p]+[c.c_int]*4+[c.c_void_p],c.c_int),
                    ('stream_probe',[c.c_void_p,c.c_void_p]+[c.c_int]*4+[c.c_void_p]*2,c.c_int),
                    ('surround_common_info',[c.c_void_p,c.c_void_p],None),
                    ('call_trace_get',[c.c_void_p],c.c_int),('opus_encoder_destroy',[c.c_void_p],None)]
        for name,types,result in signatures:
            function=getattr(lib,name);function.argtypes,function.restype=types,result
        with args.output.open('x') as output:
            output.write('# Original common-owner surround controls/calls '+REVISION+'; Q24 CELT spectral-patch locals correction only\n')
            output.write('# case RATE CHANNELS LFE APPLICATION MODE\n# frame STEP [DURATION48 for--long] REQUEST_MODE BAND BITRATE MASK_KIND INPUT_FNV SILK_RATE HB_GAIN WIDTH MODE BAND CALL_COUNT CALL7...\n')
            for rate in [16000,24000,48000]:
                for channels,lfe in [(1,0),(2,0),(1,1)]:
                    for application in [2049,2048,2051,2052,2053]:
                        for mode in range(3):
                            owner=lib.application_new(rate,channels,application,int(mode!=0))
                            if not owner:raise ValueError('Native common allocation failed')
                            output.write(f'case {rate} {channels} {lfe} {application} {mode}\n')
                            try:
                                for step in range(24):
                                    request=[1000,1000,1002,1002,1001,1001][step%6]
                                    # Native surround policy forces every coupled stream
                                    # to CELT/stereo; restricted-SILK overrides it itself.
                                    if channels==2:request=1002
                                    if rate==16000 and request==1001:request=1000
                                    band=1103 if request==1000 else 1105
                                    if lfe:band=1101
                                    bitrate=[16000,24000,32000,64000][step//6]*channels
                                    kind=step%7 if step%7<2 else 2
                                    if application==2052:kind=0
                                    mask=(c.c_int32*(21*channels))(*masks(channels,step,kind)) if kind else None
                                    if lib.surround_common_controls(owner,bitrate,int(mode!=0),int(mode==2),lfe,mask):raise ValueError('Native control failed')
                                    duration=[480,960,1920,2880,3840,4800,5760][step%7] if args.long else 960
                                    values=signal(rate,channels,duration,step,7)
                                    pcm=(c.c_int16*len(values))(*values)
                                    info,hashes=(c.c_int*4)(),(c.c_uint64*3)()
                                    n=lib.stream_probe(owner,pcm,duration*rate//48000,request,channels,band,info,hashes)
                                    if n<=0:raise ValueError(f'Native encode failed{rate}/{channels}/{application}/{mode}/{step}:{n}')
                                    observed=(c.c_int*5)();lib.surround_common_info(owner,observed)
                                    trace=(c.c_uint64*224)();count=lib.call_trace_get(trace)
                                    if count%7:raise ValueError('Invalid native trace')
                                    output.write(f'frame {step} '+(f'{duration} ' if args.long else '')+f'{request} {band} {bitrate} {kind} {fingerprint(values,"<h")} '
                                                 +' '.join(map(str,[*observed,count//7,*trace[:count]]))+'\n')
                                    rows+=1
                            finally:lib.opus_encoder_destroy(owner)
            output.write(f'# rows={rows}\n')
    if rows!=3240:raise ValueError('Incomplete protocol')
    print(f'Captured{rows}actual native common-owner mask/LFE operations',flush=True)


if __name__=='__main__':main()
