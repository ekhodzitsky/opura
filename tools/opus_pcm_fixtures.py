#!/usr/bin/env python3
"""Optional unchanged native AUDIO DC/delay-history oracle; no Cargo dependency."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION


def pcm(seed, length, pattern):
    result = []
    for i in range(length):
        seed ^= (seed << 13) & 0xffffffff
        seed ^= seed >> 17
        seed ^= (seed << 5) & 0xffffffff
        sample = (seed & 65535) - 32768
        if pattern == 0:
            sample = 0
        elif pattern == 1:
            sample = 32767 if i % 2 else -32768
        elif pattern == 2:
            sample //= 256
        result.append(sample)
    return result


def fingerprint(samples):
    value = 14695981039346656037
    for sample in samples:
        for byte in (sample & 255, (sample >> 8) & 255):
            value = ((value ^ byte) * 1099511628211) & ((1 << 64) - 1)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    text = (source/'src/opus_encoder.c').read_text()
    start = text.index('    if (st->channels*(st->encoder_buffer-(frame_size+total_buffer)) > 0)')
    end = text.index('    /* gain_fade()', start)
    history = text[start:end]

    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    wrapper = r'''
#include "opus_encoder.c"
#if !OPUS_FAST_INT64 || DB_SHIFT != 24
#error "Expected scalar fast64 fixed-point Q24 reference"
#endif
void *pcm_new(int Fs,int channels) {
 int error;
 OpusEncoder *st=opus_encoder_create(Fs,channels,OPUS_APPLICATION_AUDIO,&error);
 return error ? NULL : st;
}
void pcm_probe(OpusEncoder *st,const opus_int16 *input,int frame_size,int advance,
               opus_int16 *filtered,opus_int16 *aligned,opus_int16 *history,int *dc) {
 int total_buffer=st->delay_compensation;
 opus_int16 pcm_buf[120*48*2+384];
 OPUS_COPY(pcm_buf,&st->delay_buffer[(st->encoder_buffer-total_buffer)*st->channels],total_buffer*st->channels);
 dc_reject(input,3,&pcm_buf[total_buffer*st->channels],st->hp_mem,frame_size,st->channels,st->Fs);
 OPUS_COPY(filtered,&pcm_buf[total_buffer*st->channels],frame_size*st->channels);
 OPUS_COPY(aligned,pcm_buf,frame_size*st->channels);
 if (advance) {
HISTORY
 }
 OPUS_COPY(history,st->delay_buffer,st->encoder_buffer*st->channels);
 dc[0]=st->hp_mem[0];dc[1]=st->hp_mem[2];
}
'''.replace('HISTORY', history)
    with tempfile.TemporaryDirectory(prefix='opura-pcm-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(wrapper)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files if f != 'src/opus_encoder.c'],
                        '-lm', '-o', str(folder/'probe.so')], check=True)
        lib = c.CDLL(str(folder/'probe.so'))
        lib.pcm_new.argtypes = [c.c_int, c.c_int]
        lib.pcm_new.restype = c.c_void_p
        lib.opus_encoder_destroy.argtypes = [c.c_void_p]
        lib.opus_encoder_destroy.restype = None
        ptr = c.POINTER(c.c_int16)
        lib.pcm_probe.argtypes = [c.c_void_p, ptr, c.c_int, c.c_int, ptr, ptr, ptr, c.POINTER(c.c_int)]
        lib.pcm_probe.restype = None
        durations = [120, 240, 480, 960, 1920, 2880, 3840, 4800, 5760]
        rows = ['# Opus '+REVISION+' unchanged native DC and delay-buffer update block; fixed scalar Q24',
                '# RATE CHANNELS INDEX SAMPLES48 ADVANCE SEED PATTERN DC0 DC1 FILTER_FNV64 ALIGNED_FNV64 HISTORY_FNV64']
        for rate in [8000, 12000, 16000, 24000, 48000]:
            for channels in [1, 2]:
                state = lib.pcm_new(rate, channels)
                if not state:
                    raise ValueError('Cannot create reference encoder')
                try:
                    for i in range(144):
                        # Include repeated short frames, long->short and history
                        # retention while filtering continues during early DTX.
                        size = durations[i % 9 if i < 72 else (i // 9) % 9]
                        count = size * rate // 48000 * channels
                        seed = (0x6f707573 + i*982451653 + rate*channels) & 0xffffffff
                        pattern = (i // 3) % 4
                        advance = int(i % 13 not in [5, 6, 7])
                        samples = (c.c_int16 * count)(*pcm(seed, count, pattern))
                        filtered, aligned = (c.c_int16 * count)(), (c.c_int16 * count)()
                        saved, dc = (c.c_int16 * (rate//100*channels))(), (c.c_int * 2)()
                        lib.pcm_probe(state, samples, size*rate//48000, advance, filtered, aligned, saved, dc)
                        row = [rate, channels, i, size, advance, seed, pattern, *dc,
                               fingerprint(filtered), fingerprint(aligned), fingerprint(saved)]
                        rows.append(' '.join(map(str, row)))
                finally:
                    lib.opus_encoder_destroy(state)
        args.output.write_text('\n'.join(rows)+'\n')
        print('Captured 1440 unchanged native PCM/filter/history snapshots')


if __name__ == '__main__':
    main()
