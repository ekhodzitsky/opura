#!/usr/bin/env python3
"""Trace unchanged public SILK noise shaping, spectral input and gain processing.

Optional native oracle only: ordinary Rust builds/tests have no C dependency.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import DRIVER, REVISION
from silk_pitch_analysis_fixtures import WRAPPER as PITCH_WRAPPER

WRAPPER = PITCH_WRAPPER.replace('static void emit(FILE *out,int first)', 'static void emit_pitch(FILE *out,int first)') + r'''
static struct {
 int rate,count,complexity,snr,cbr,activity,tilt,quality[4],before[2],after[2],input_quality,coding_quality,offset,order,length;
 opus_int16 input[720],ar[96];opus_int32 gains[4],lf[4];int harmonic[4],noise_tilt[4];
 struct {int minimum,use,first,survivors;opus_int16 previous[16];}s;
 struct {int conditional,previous,after,offset,lambda,ltp_gain;opus_int32 before[4],unquantized[4],after_gains[4],energy[4],q[4];opus_int8 indices[4];}g;
}analysis;
void __real_silk_noise_shape_analysis_FIX(silk_encoder_state_FIX*,silk_encoder_control_FIX*,const opus_int16*,const opus_int16*,int);
void __wrap_silk_noise_shape_analysis_FIX(silk_encoder_state_FIX *s,silk_encoder_control_FIX *c,const opus_int16 *res,const opus_int16 *x,int arch){
 analysis.rate=s->sCmn.fs_kHz;analysis.count=s->sCmn.nb_subfr;analysis.complexity=s->sCmn.Complexity;analysis.snr=s->sCmn.SNR_dB_Q7;analysis.cbr=s->sCmn.useCBR;
 analysis.activity=s->sCmn.speech_activity_Q8;analysis.tilt=s->sCmn.input_tilt_Q15;
 for(int i=0;i<4;i++)analysis.quality[i]=s->sCmn.input_quality_bands_Q15[i];
 analysis.before[0]=s->sShape.HarmShapeGain_smth_Q16;analysis.before[1]=s->sShape.Tilt_smth_Q16;
 analysis.length=s->sCmn.ltp_mem_length+s->sCmn.frame_length+s->sCmn.la_shape;
 memcpy(analysis.input,x-s->sCmn.ltp_mem_length,analysis.length*sizeof(*x));
 __real_silk_noise_shape_analysis_FIX(s,c,res,x,arch);
 analysis.after[0]=s->sShape.HarmShapeGain_smth_Q16;analysis.after[1]=s->sShape.Tilt_smth_Q16;
 analysis.input_quality=c->input_quality_Q14;analysis.coding_quality=c->coding_quality_Q14;analysis.offset=s->sCmn.indices.quantOffsetType;analysis.order=s->sCmn.shapingLPCOrder;
 memcpy(analysis.ar,c->AR_Q13,sizeof(analysis.ar));memcpy(analysis.gains,c->Gains_Q16,sizeof(analysis.gains));memcpy(analysis.lf,c->LF_shp_Q14,sizeof(analysis.lf));
 for(int i=0;i<4;i++){analysis.harmonic[i]=c->HarmShapeGain_Q14[i];analysis.noise_tilt[i]=c->Tilt_Q14[i];}
}
void __real_silk_find_LPC_FIX(silk_encoder_state*,opus_int16*,const opus_int16*,opus_int32);
void __wrap_silk_find_LPC_FIX(silk_encoder_state *s,opus_int16 *out,const opus_int16 *in,opus_int32 minimum){
 analysis.s.minimum=minimum;analysis.s.use=s->useInterpolatedNLSFs;analysis.s.first=s->first_frame_after_reset;analysis.s.survivors=s->NLSF_MSVQ_Survivors;
 memcpy(analysis.s.previous,s->prev_NLSFq_Q15,s->predictLPCOrder*sizeof(*in));
 __real_silk_find_LPC_FIX(s,out,in,minimum);
}
void __real_silk_process_gains_FIX(silk_encoder_state_FIX*,silk_encoder_control_FIX*,opus_int);
void __wrap_silk_process_gains_FIX(silk_encoder_state_FIX *s,silk_encoder_control_FIX *c,opus_int conditional){
 analysis.g.conditional=conditional;analysis.g.previous=s->sShape.LastGainIndex;analysis.g.ltp_gain=c->LTPredCodGain_Q7;
 memcpy(analysis.g.before,c->Gains_Q16,sizeof(analysis.g.before));memcpy(analysis.g.energy,c->ResNrg,sizeof(analysis.g.energy));
 for(int i=0;i<4;i++)analysis.g.q[i]=c->ResNrgQ[i];
 __real_silk_process_gains_FIX(s,c,conditional);
 analysis.g.after=s->sShape.LastGainIndex;analysis.g.offset=s->sCmn.indices.quantOffsetType;analysis.g.lambda=c->Lambda_Q10;
 memcpy(analysis.g.unquantized,c->GainsUnq_Q16,sizeof(analysis.g.unquantized));memcpy(analysis.g.after_gains,c->Gains_Q16,sizeof(analysis.g.after_gains));memcpy(analysis.g.indices,s->sCmn.indices.GainsIndices,4);
}
static void emit(FILE *out,int first){
 emit_pitch(out,first);
 fprintf(out,"shape %d %d %d %d %d %d %d",analysis.rate,analysis.count,analysis.complexity,analysis.snr,analysis.cbr,analysis.activity,analysis.tilt);
 for(int i=0;i<4;i++)fprintf(out," %d",analysis.quality[i]);
 for(int i=0;i<2;i++)fprintf(out," %d",analysis.before[i]);
 for(int i=0;i<2;i++)fprintf(out," %d",analysis.after[i]);
 fprintf(out," %d %d %d %d %d",analysis.input_quality,analysis.coding_quality,analysis.offset,analysis.order,analysis.length);
 for(int i=0;i<analysis.length;i++)fprintf(out," %d",analysis.input[i]);
 for(int k=0;k<analysis.count;k++)for(int i=0;i<analysis.order;i++)fprintf(out," %d",analysis.ar[k*24+i]);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.gains[i]);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.lf[i]);
 for(int i=0;i<4;i++)fprintf(out," %d",analysis.harmonic[i]);
 for(int i=0;i<4;i++)fprintf(out," %d",analysis.noise_tilt[i]);fprintf(out,"\n");
 fprintf(out,"spectral %d %d %d %d",analysis.s.minimum,analysis.s.use,analysis.s.first,analysis.s.survivors);
 int order=analysis.rate==16?16:10;for(int i=0;i<order;i++)fprintf(out," %d",analysis.s.previous[i]);fprintf(out,"\n");
 fprintf(out,"process %d %d %d %d %d %d",analysis.g.conditional,analysis.g.previous,analysis.g.after,analysis.g.offset,analysis.g.lambda,analysis.g.ltp_gain);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.g.before[i]);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.g.energy[i]);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.g.q[i]);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.g.unquantized[i]);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.g.after_gains[i]);
 for(int i=0;i<analysis.count;i++)fprintf(out," %d",analysis.g.indices[i]);fprintf(out,"\n");
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
        raise SystemExit('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M|re.S)
        if match is None:
            raise SystemExit('Missing source manifest')
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    driver = '\n'.join(line for line in DRIVER.splitlines() if not line.lstrip().startswith('fprintf(out,"#'))
    driver = driver.replace('if(!out)return 2;', 'if(!out)return 2;\nfprintf(out,"# Public native SILK shaping '+REVISION+'\\n");')
    driver = driver.replace('OPUS_SET_COMPLEXITY(0)', 'OPUS_SET_COMPLEXITY(bandwidth==1101?0:bandwidth==1102?6:10)')
    driver = driver.replace('for(int bandwidth=1101;', 'for(int vbr=0;vbr<=1;vbr++)for(int bandwidth=1101;')
    driver = driver.replace('OPUS_SET_VBR(0)', 'OPUS_SET_VBR(vbr)')
    names = ['silk_find_pitch_lags_FIX', 'silk_quant_LTP_gains', 'silk_LTP_analysis_filter_FIX',
             'silk_noise_shape_analysis_FIX', 'silk_find_LPC_FIX', 'silk_process_gains_FIX']
    with tempfile.TemporaryDirectory(prefix='opura-silk-shaping-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER+'\n'+driver)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files],
                        *['-Wl,--wrap='+name for name in names], '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured 144 CBR/VBR native pitch/shaping/LTP/spectral/gain groups')


if __name__ == '__main__':
    main()
