#!/usr/bin/env python3
"""Optional pristine/corrected-Q24 native surround-analysis oracle.

Both builds use separate temporary copies of the original translation unit.
The original checkout and all codec/decoder sources remain unchanged. Corrected
analysis retains logSum's Q24 return and scales channel_offset into Q24. Foreign
code is never a Cargo or ordinary-test dependency.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import struct
import subprocess
import tempfile
from audio_baseline import CFLAGS, REVISION, run

DURATIONS = [120, 240, 480, 960, 1920, 2880, 3840, 4800, 5760]
CORRECTIONS = [
    ('static opus_val16 logSum(celt_glog a, celt_glog b)',
     'static celt_glog logSum(celt_glog a, celt_glog b)'),
    ('opus_val16 channel_offset;', 'celt_glog channel_offset;'),
    ('channel_offset = HALF16(celt_log2(QCONST32(2.f,14)/(channels-1)));',
     'channel_offset = HALF32(SHL32(EXTEND32(celt_log2(QCONST32(2.f,14)/(channels-1))),DB_SHIFT-10));'),
]
WRAPPER = r'''
#if !OPUS_FAST_INT64 || DB_SHIFT != 24
#error "Expected classic fixed fast64 Q24"
#endif
int opura_surround(const opus_int16 *pcm, int channels, int rate, int duration,
                  celt_glog *logs, opus_val32 *memory, opus_val32 *preemph) {
 int error=0;
 const CELTMode *mode=NULL;
 OpusEncoder *encoder=opus_encoder_create(48000,1,OPUS_APPLICATION_AUDIO,&error);
 if(!encoder || error) return -1;
 if(opus_encoder_ctl(encoder,CELT_GET_MODE(&mode))) { opus_encoder_destroy(encoder);return -2; }
 surround_analysis(mode,pcm,logs,memory,preemph,duration*rate/48000,
                   mode->overlap,channels,rate,opus_copy_channel_in_short,0);
 opus_encoder_destroy(encoder);
 return 0;
}
int opura_logsum(int a,int b) { return logSum(a,b); }
'''


def compile_oracle(source, directory, corrected):
    if run(['git', '-C', source, 'rev-parse', 'HEAD']).strip() != REVISION:
        raise ValueError('Expected pinned original native revision')
    if run(['git', '-C', source, 'status', '--porcelain']).strip():
        raise ValueError('Native reference checkout must be unchanged')

    def manifest(file, name):
        match = re.search(r'^' + name + r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED')
             + manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    original = (source/'src/opus_multistream_encoder.c').read_text()
    if corrected:
        for old, new in CORRECTIONS:
            if original.count(old) != 1:
                raise ValueError('Q24 correction target changed')
            original = original.replace(old, new)
    wrapper = directory/('surround-q24.c' if corrected else 'surround-pristine.c')
    encoder = (source/'celt/celt_encoder.c').read_text()
    begin = encoder.index('   /* This computes how much masking takes place between surround channels */')
    end = encoder.index('   /* Temporal VBR (but not for LFE) */', begin)
    block = encoder[begin:end]
    reducer = r'''
int opura_surround_reduce(int channels,int coded,celt_glog *mask,celt_glog *out) {
 int error=0;
 const CELTMode *mode=NULL;
 OpusEncoder *encoder=opus_encoder_create(48000,1,OPUS_APPLICATION_AUDIO,&error);
 if(!encoder || error) return -1;
 if(opus_encoder_ctl(encoder,CELT_GET_MODE(&mode))) { opus_encoder_destroy(encoder);return -2; }
 const opus_int16 *eBands=mode->eBands;
 struct {int lastCodedBands,lfe;celt_glog *energy_mask;} state={coded,0,mask},*st=&state;
 int c,i,C=channels,nbEBands=21,hybrid=0;
 celt_glog surround_trim=0,surround_masking=0,surround_dynalloc[42]={0};
BLOCK
 out[0]=surround_trim;out[1]=surround_masking;
 for(i=0;i<21;i++)out[i+2]=surround_dynalloc[i];
 opus_encoder_destroy(encoder);
 return 0;
}
'''.replace('BLOCK', block)
    wrapper.write_text(original + WRAPPER + reducer)
    library = wrapper.with_suffix('.so')
    includes = ['-I' + str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']]
    subprocess.run(['cc', *CFLAGS, *includes, str(wrapper),
                    *[str(source/f) for f in files if f != 'src/opus_multistream_encoder.c'],
                    '-lm', '-o', str(library)], check=True)
    lib = c.CDLL(str(library))
    lib.opura_surround.argtypes = [c.c_void_p, c.c_int, c.c_int, c.c_int, c.c_void_p, c.c_void_p, c.c_void_p]
    lib.opura_surround.restype = c.c_int
    lib.opura_logsum.argtypes, lib.opura_logsum.restype = [c.c_int, c.c_int], c.c_int
    lib.opura_surround_reduce.argtypes = [c.c_int, c.c_int, c.c_void_p, c.c_void_p]
    lib.opura_surround_reduce.restype = c.c_int
    return lib


def fingerprint(values, fmt):
    result = 14695981039346656037
    for value in values:
        for byte in struct.pack(fmt, value):
            result = ((result ^ byte) * 1099511628211) & 0xffffffffffffffff
    return result


def signal(rate, channels, duration, step, kind):
    count = duration * rate // 48000
    result = []
    seed = (6716 + 37 * step) & 0xffffffff
    for frame in range(count):
        for channel in range(channels):
            seed = (1664525 * seed + 1013904223) & 0xffffffff
            phase = (frame + step * 13 + channel * 37) % (31 + 11 * channel)
            value = (phase - (15 + 5 * channel)) * 173
            if kind == 0:
                value = 0
            elif kind == 1:
                value = 30000 if frame == count // 2 + channel else 0
            elif kind == 2:
                value = (seed >> 16) - 32768
            elif kind == 3:
                value = 20000 if phase % 9 < 4 else -20000
            elif kind == 4:
                value = value if channel == 1 else 0
            elif kind == 5:
                value = 32767 if (frame + channel) % 2 else -32768
            elif kind == 6:
                value = value // 1024
            else:
                value += ((seed >> 24) - 128) * 3
            result.append(value)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--allocation-output', type=Path)
    args = parser.parse_args()
    if args.output.exists() or (args.allocation_output is not None and args.allocation_output.exists()):
        raise ValueError('Output must be new')
    rows = differences = 0
    with tempfile.TemporaryDirectory(prefix='opura-surround-analysis-') as temporary:
        directory = Path(temporary)
        pristine = compile_oracle(args.source.resolve(), directory, False)
        corrected = compile_oracle(args.source.resolve(), directory, True)
        print(f'logSum(0,0): pristine={pristine.opura_logsum(0,0)} corrected={corrected.opura_logsum(0,0)}', flush=True)
        if pristine.opura_logsum(0, 0) != 0 or corrected.opura_logsum(0, 0) != 1 << 23:
            raise ValueError('Expected observed Q24 narrowing boundary')
        with args.output.open('x') as out:
            out.write('# Native surround analysis ' + REVISION + '\n')
            out.write('# Expected masks use the three explicit Q24 corrections in tools/surround_analysis_fixtures.py\n')
            out.write('# Pristine output hashes are retained separately; original native checkout and decoder unchanged\n')
            out.write('# case RATE CHANNELS\n# frame DURATION48 RESET STEP KIND INPUT_FNV OVERLAP_FNV PRISTINE_MASK_FNV PREEMPH... MASKS_CHANNEL_MAJOR...\n')
            for rate in [8000, 12000, 16000, 24000, 48000]:
                for channels in range(3, 9):
                    memory = (c.c_int32 * (120 * channels))()
                    preemph = (c.c_int32 * channels)()
                    old_memory = (c.c_int32 * (120 * channels))()
                    old_preemph = (c.c_int32 * channels)()
                    out.write(f'case {rate} {channels}\n')
                    for step, (duration, kind) in enumerate((duration, kind) for duration in DURATIONS for kind in range(8)):
                        reset = int(step % 13 == 0)
                        if reset:
                            memory[:] = old_memory[:] = [0] * (120 * channels)
                            preemph[:] = old_preemph[:] = [0] * channels
                        values = signal(rate, channels, duration, step, kind)
                        pcm = (c.c_int16 * len(values))(*values)
                        logs, old_logs = (c.c_int32 * (21 * channels))(), (c.c_int32 * (21 * channels))()
                        for lib, result, history, pre in [(pristine, old_logs, old_memory, old_preemph), (corrected, logs, memory, preemph)]:
                            if lib.opura_surround(pcm, channels, rate, duration, result, history, pre):
                                raise ValueError('Native analysis failed')
                        if memory[:] != old_memory[:] or preemph[:] != old_preemph[:]:
                            raise ValueError('Q24 correction unexpectedly changes PCM histories')
                        differences += logs[:] != old_logs[:]
                        out.write(f'frame {duration} {reset} {step} {kind} {fingerprint(values,"<h")} {fingerprint(memory,"<i")} {fingerprint(old_logs,"<i")} '
                                  + ' '.join(map(str, [*preemph, *logs])) + '\n')
                        rows += 1
            out.write(f'# rows={rows} pristine_different_rows={differences}\n')
        if args.allocation_output is not None:
            with args.allocation_output.open('x') as out:
                out.write('# Original unmodified CELT surround allocation mask reduction ' + REVISION + '\n')
                out.write('# CHANNELS LAST_CODED INPUT_MASKS_CHANNEL_MAJOR... TRIM_Q24 MASKING_Q24 DYNALLOC_Q24...\n')
                allocation_rows = 0
                for channels in [1, 2]:
                    for coded in range(22):
                        for kind in range(64):
                            values = []
                            for channel in range(channels):
                                for band in range(21):
                                    if kind == 0:
                                        value = 0
                                    elif kind == 1:
                                        value = -(2 << 24)
                                    elif kind == 2:
                                        value = (1 << 22)
                                    elif kind == 3:
                                        value = -(32 << 24) if band % 2 else 32 << 24
                                    else:
                                        value = ((kind * 719 + band * 317 + channel * 517) % (8 * 4096) - 4 * 4096) * 4096
                                    values.append(value)
                            mask = (c.c_int32 * len(values))(*values)
                            result = (c.c_int32 * 23)()
                            original_result = (c.c_int32 * 23)()
                            if corrected.opura_surround_reduce(channels, coded, mask, result) or pristine.opura_surround_reduce(channels, coded, mask, original_result):
                                raise ValueError('Native mask reduction failed')
                            if result[:] != original_result[:]:
                                raise ValueError('Analysis correction unexpectedly changes mask reduction')
                            out.write(' '.join(map(str, [channels, coded, *values, *result])) + '\n')
                            allocation_rows += 1
                out.write(f'# rows={allocation_rows}\n')
            print(f'Captured{allocation_rows}unchanged native surround allocation cases', flush=True)
    if rows != 2160 or not differences:
        raise ValueError('Incomplete analysis or unobserved Q24 correction')
    print(f'Captured{rows}native surround analysis cases;{differences}differ from pristine narrowing', flush=True)


if __name__ == '__main__':
    main()
