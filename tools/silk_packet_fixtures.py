#!/usr/bin/env python3
"""Generate real mono SILK packets and internal PCM using pinned fixed-point Opus.

Optional developer oracle: compiles the external reference into a temporary
library. The only instrumentation copies decode_core output after the original
routine returns, optionally copies completed decode_frame output, and observes
each frame's transition-redundancy decision;
opus_decode still runs the complete upstream decoder and its
public OPUS_GET_FINAL_RANGE result is recorded independently.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('--loss', action='store_true', help='Emit loss/recovery streams with completed decode_frame PCM')
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected Opus v1.6.1 at ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')


def manifest(file, variable):
    text = (source / file).read_text()
    match = re.search(r'^' + variable + r' = (.*?)(?=\n\n|\Z)', text, re.M | re.S)
    if match is None:
        raise SystemExit('Missing source manifest ' + variable)
    return match[1].replace('\\', '').split()


wrapper = r'''
#include "main.h"
#include "opus.h"
#include "opus_private.h"
static opus_int16 captured[1920];
static int captured_length, captured_rate;
static int transition_seen;
static opus_int16 finished[1920];
static int finished_length;
int reference_decode_frame(silk_decoder_state *, ec_dec *, opus_int16 *, opus_int32 *, int, int, int);
int silk_decode_frame(silk_decoder_state *state, ec_dec *decoder, opus_int16 *pcm,
                      opus_int32 *length, int lost, int coding, int arch) {
    int result = reference_decode_frame(state, decoder, pcm, length, lost, coding, arch);
    if (result == 0) {
        if (finished_length + *length > 1920) abort();
        memcpy(finished + finished_length, pcm, *length * sizeof(*pcm));
        finished_length += *length;
        captured_rate = state->fs_kHz;
    }
    return result;
}
void record_transition(int present) { transition_seen |= present; }
void reference_decode_core(silk_decoder_state *, silk_decoder_control *, opus_int16 *, const opus_int16 *, int);
void silk_decode_core(silk_decoder_state *state, silk_decoder_control *control,
                      opus_int16 *pcm, const opus_int16 *pulses, int arch) {
    reference_decode_core(state, control, pcm, pulses, arch);
    if (captured_length + state->frame_length > 1920) abort();
    memcpy(captured + captured_length, pcm, state->frame_length * sizeof(*pcm));
    captured_length += state->frame_length;
    captured_rate = state->fs_kHz;
}
/* Synthetic symbol packets complement real audio with every sparse LBRR mask. */
int symbols(int rate, int duration, int mask, unsigned char *packet) {
    silk_encoder_state state = {0};
    ec_enc ec;
    state.fs_kHz = rate;
    state.nb_subfr = duration == 10 ? 2 : 4;
    state.frame_length = rate * (duration == 10 ? 10 : 20);
    state.predictLPCOrder = rate == 16 ? 16 : 10;
    state.psNLSF_CB = rate == 16 ? &silk_NLSF_CB_WB : &silk_NLSF_CB_NB_MB;
    state.pitch_lag_low_bits_iCDF = rate == 8 ? silk_uniform4_iCDF : rate == 12 ? silk_uniform6_iCDF : silk_uniform8_iCDF;
    state.pitch_contour_iCDF = rate == 8
        ? (duration == 10 ? silk_pitch_contour_10_ms_NB_iCDF : silk_pitch_contour_NB_iCDF)
        : (duration == 10 ? silk_pitch_contour_10_ms_iCDF : silk_pitch_contour_iCDF);
    int frames = duration <= 20 ? 1 : duration / 20;
    ec_enc_init(&ec, packet + 1, 1275);
    for (int i = 0; i < frames; i++) ec_enc_bit_logp(&ec, 1, 1);
    ec_enc_bit_logp(&ec, mask != 0, 1);
    if (mask && frames > 1) ec_enc_icdf(&ec, mask - 1, silk_LBRR_flags_iCDF_ptr[frames - 2], 8);
    for (int redundant = 1; redundant >= 0; redundant--) {
        for (int i = 0; i < frames; i++) {
            if (redundant && !(mask & (1 << i))) continue;
            int conditional = i > 0 && (!redundant || (mask & (1 << (i - 1))));
            SideInfoIndices *indices = redundant ? &state.indices_LBRR[i] : &state.indices;
            memset(indices, 0, sizeof(*indices));
            indices->signalType = TYPE_VOICED;
            indices->GainsIndices[0] = conditional ? 4 : redundant ? 42 : 10;
            for (int k = 1; k < state.nb_subfr; k++) indices->GainsIndices[k] = 4;
            indices->NLSFIndices[0] = redundant ? 17 : 0;
            indices->NLSFInterpCoef_Q2 = duration == 10 ? 4 : 2;
            indices->lagIndex = 20;
            indices->Seed = i;
            silk_encode_indices(&state, &ec, i, redundant, conditional ? CODE_CONDITIONALLY : CODE_INDEPENDENTLY);
            opus_int8 pulses[320] = {0};
            for (int k = 0; k < state.frame_length; k += 7) pulses[k] = k % 2 ? 1 : -1;
            silk_encode_pulses(&ec, indices->signalType, 0, pulses, state.frame_length);
        }
    }
    int bytes = (ec_tell(&ec) + 7) / 8;
    ec_enc_shrink(&ec, bytes);
    ec_enc_done(&ec);
    if (ec_get_error(&ec)) return OPUS_INTERNAL_ERROR;
    packet[0] = ((rate == 8 ? 0 : rate == 12 ? 4 : 8) + (duration == 10 ? 0 : duration == 20 ? 1 : duration == 40 ? 2 : 3)) << 3;
    return bytes + 1;
}
int configure(OpusEncoder *encoder, int bandwidth, int fec) {
    int ret = 0;
    ret |= opus_encoder_ctl(encoder, OPUS_SET_FORCE_MODE(MODE_SILK_ONLY));
    ret |= opus_encoder_ctl(encoder, OPUS_SET_BANDWIDTH(bandwidth));
    ret |= opus_encoder_ctl(encoder, OPUS_SET_MAX_BANDWIDTH(bandwidth));
    ret |= opus_encoder_ctl(encoder, OPUS_SET_BITRATE(18000));
    ret |= opus_encoder_ctl(encoder, OPUS_SET_COMPLEXITY(5));
    ret |= opus_encoder_ctl(encoder, OPUS_SET_INBAND_FEC(fec));
    ret |= opus_encoder_ctl(encoder, OPUS_SET_PACKET_LOSS_PERC(fec ? 30 : 0));
    return ret;
}
int repack(const unsigned char *packets, const int *lengths, int count, unsigned char *out) {
    OpusRepacketizer *rp = opus_repacketizer_create();
    if (!rp) return OPUS_ALLOC_FAIL;
    int offset = 0, result = OPUS_OK;
    for (int i = 0; i < count && result == OPUS_OK; i++) {
        result = opus_repacketizer_cat(rp, packets + offset, lengths[i]);
        offset += lengths[i];
    }
    if (result == OPUS_OK) result = opus_repacketizer_out(rp, out, 1276);
    opus_repacketizer_destroy(rp);
    return result;
}
int decode(OpusDecoder *decoder, const unsigned char *packet, int len,
           opus_int16 *raw, opus_uint32 *out) {
    opus_int16 pcm[5760];
    captured_length = 0;
    finished_length = 0;
    transition_seen = 0;
    int result = opus_decode(decoder, packet, len, pcm, 5760, 0);
    if (result < 0) return result;
    memcpy(raw, captured, captured_length * sizeof(*raw));
    out[0] = captured_length;
    out[1] = captured_rate;
    if (opus_decoder_ctl(decoder, OPUS_GET_FINAL_RANGE(out + 2)) != OPUS_OK) abort();
    out[3] = transition_seen;
    return result;
 }
int decode_complete(OpusDecoder *decoder, const unsigned char *packet, int len, int frame_samples,
                    opus_int16 *raw, opus_uint32 *out) {
    opus_int16 pcm[5760];
    captured_length = 0;
    finished_length = 0;
    transition_seen = 0;
    int result = opus_decode(decoder, packet, len, pcm, frame_samples, 0);
    if (result < 0) return result;
    memcpy(raw, finished, finished_length * sizeof(*raw));
    out[0] = finished_length;
    out[1] = captured_rate;
    if (opus_decoder_ctl(decoder, OPUS_GET_FINAL_RANGE(out + 2)) != OPUS_OK) abort();
    out[3] = transition_seen;
    return result;
}
'''
files = (manifest('silk_sources.mk', 'SILK_SOURCES')
         + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED')
         + manifest('celt_sources.mk', 'CELT_SOURCES')
         + manifest('opus_sources.mk', 'OPUS_SOURCES'))
with tempfile.TemporaryDirectory(prefix='opura-packet-reference-') as temp:
    path = Path(temp)
    # Preserve the original file, including its license; rename only its entry
    # point so the wrapper can capture output without changing codec arithmetic.
    core = (source / 'silk/decode_core.c').read_text()
    if core.count('void silk_decode_core(') != 1:
        raise SystemExit('Unexpected reference decode_core signature')
    (path / 'core.c').write_text(core.replace('void silk_decode_core(', 'void reference_decode_core('))
    frame_source = (source / 'silk/decode_frame.c').read_text()
    if frame_source.count('opus_int silk_decode_frame(') != 1:
        raise SystemExit('Unexpected reference decode_frame signature')
    (path / 'frame.c').write_text(frame_source.replace('opus_int silk_decode_frame(', 'opus_int reference_decode_frame('))
    (path / 'probe.c').write_text(wrapper)
    decoder_source = (source / 'src/opus_decoder.c').read_text()
    anchor = '   if (mode != MODE_CELT_ONLY)\n      start_band = 17;'
    if decoder_source.count(anchor) != 1:
        raise SystemExit('Unexpected transition decision site')
    decoder_source = decoder_source.replace('#include "opus_private.h"',
        '#include "opus_private.h"\nextern void record_transition(int);')
    (path / 'decoder.c').write_text(decoder_source.replace(anchor,
        '   record_transition(redundancy);\n' + anchor))
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                    str(path / 'probe.c'), str(path / 'core.c'), str(path / 'decoder.c'), str(path / 'frame.c'),
                    *[str(source / f) for f in files if f not in ['silk/decode_core.c', 'src/opus_decoder.c', 'silk/decode_frame.c']],
                    '-lm', '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    pointer, i16, u8, u32 = c.c_void_p, c.POINTER(c.c_int16), c.POINTER(c.c_ubyte), c.POINTER(c.c_uint32)
    for name, types, result in [
        ('opus_encoder_create', [c.c_int, c.c_int, c.c_int, c.POINTER(c.c_int)], pointer),
        ('opus_decoder_create', [c.c_int, c.c_int, c.POINTER(c.c_int)], pointer),
        ('opus_encoder_destroy', [pointer], None), ('opus_decoder_destroy', [pointer], None),
        ('configure', [pointer, c.c_int, c.c_int], c.c_int),
        ('symbols', [c.c_int, c.c_int, c.c_int, u8], c.c_int),
        ('opus_encode', [pointer, i16, c.c_int, u8, c.c_int], c.c_int),
        ('decode', [pointer, u8, c.c_int, i16, u32], c.c_int),
        ('decode_complete', [pointer, u8, c.c_int, c.c_int, i16, u32], c.c_int),
        ('repack', [u8, c.POINTER(c.c_int), c.c_int, u8], c.c_int),
    ]:
        function = getattr(lib, name)
        function.argtypes, function.restype = types, result
    if args.loss:
        print('# Pinned Opus v1.6.1 real mono SILK loss/recovery, completed decode_frame PCM:', REVISION)
        print('# KIND RESET DURATION_MS HEX RATE_KHZ FINAL_RANGE PCM[duration_ms*rate]')
        error = c.c_int()
        for sequence in range(6):
            encoder = lib.opus_encoder_create(48000, 1, 2048, c.byref(error))
            if not encoder or error.value: raise SystemExit('Encoder initialization failed')
            decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
            if not decoder or error.value: raise SystemExit('Decoder initialization failed')
            phase, random = 0, 0x6c6f7373 + sequence
            has_history, reset_next = False, True
            try:
                for index in range(48):
                    if lib.configure(encoder, 1101 + sequence // 2, sequence % 2):
                        raise SystemExit('Encoder configuration failed')
                    duration = [10, 20, 40, 60][index // 12]
                    samples = []
                    for _ in range(48 * duration):
                        phase = (phase + 1) % 240
                        random ^= (random << 13) & 0xffffffff
                        random ^= random >> 17
                        random ^= (random << 5) & 0xffffffff
                        value = ((phase if phase < 120 else 239 - phase) - 60) * 160
                        samples.append(0 if index % 16 >= 10 else value + random % 1024 - 512)
                    data = (c.c_ubyte * 1276)()
                    size = lib.opus_encode(encoder, (c.c_int16 * len(samples))(*samples), len(samples), data, 1276)
                    if size <= 1: raise SystemExit('Reference audio encoding failed')
                    packet = bytes(data[:size])
                    if packet[0] >> 3 >= 12: raise SystemExit('Expected SILK mono')
                    lost = has_history and index % 12 in [2, 3, 4, 7, 10]
                    if lost: duration = 10 * (1 + (index + sequence) % 12)
                    pcm, out = (c.c_int16 * 1920)(), (c.c_uint32 * 4)()
                    result = lib.decode_complete(decoder, None if lost else data, 0 if lost else size,
                                                 48 * duration, pcm, out)
                    if result != 48 * duration or not out[0]: raise SystemExit('Reference loss/recovery decode failed')
                    kind = 'transition' if out[3] else 'loss' if lost else 'packet'
                    print(kind, int(reset_next), duration, '-' if lost else packet.hex(), out[1], out[2], *pcm[:out[0]])
                    reset_next = bool(out[3])
                    has_history = not reset_next
                    if reset_next:
                        lib.opus_decoder_destroy(decoder)
                        decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
                        if not decoder or error.value: raise SystemExit('Decoder reset failed')
            finally:
                lib.opus_encoder_destroy(encoder)
                lib.opus_decoder_destroy(decoder)
        raise SystemExit(0)
    print('# Opus v1.6.1 fixed-point real mono audio packets:', REVISION)
    print('# Scalar fixed-point -O2 -fwrapv, no deep PLC/OSCE; integer deterministic source at 48 kHz.')
    print('# KIND RESET HEX RATE_KHZ FINAL_RANGE PCM[duration_ms*rate]; raw decode_core output before mono buffering/resampling.')
    error = c.c_int()
    for sequence in range(8):
        encoder = lib.opus_encoder_create(48000, 1, 2048, c.byref(error))
        if not encoder or error.value: raise SystemExit('Encoder initialization failed')
        decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
        if not decoder or error.value: raise SystemExit('Decoder initialization failed')
        history = []
        reset_next = True
        phase, random = 0, 0x61756469 + sequence
        try:
            for index in range(24 if sequence < 6 else 42):
                # Fixed-rate sequences and one shared-state NB->MB->WB->MB->NB
                # sequence exercise actual signaled transitions, not TOC edits.
                band = sequence // 2 if sequence < 5 else [0, 1, 2, 1, 0, 2][(index // 4) % 6]
                if sequence >= 6:
                    band = 0 if sequence == 6 else 2
                if lib.configure(encoder, 1101 + band, sequence % 2 if sequence < 6 else 1):
                    raise SystemExit('Reference encoder configuration failed')
                duration = [10, 20, 40, 60][(index // 3) % 4]
                if sequence >= 6:
                    duration = 40 if sequence == 6 else 60
                length = 48 * duration
                samples = []
                for _ in range(length):
                    phase = (phase + 1) % 240
                    random ^= (random << 13) & 0xffffffff
                    random ^= random >> 17
                    random ^= (random << 5) & 0xffffffff
                    # Periodic voiced-like excitation, shaped noise, and silence.
                    amplitude = int(index % 14 >= 8) if sequence >= 6 else int(index % 12 < 10)
                    value = ((phase if phase < 120 else 239 - phase) - 60) * 160
                    samples.append(amplitude * (value + (random % 1024) - 512))
                data = (c.c_ubyte * 1276)()
                size = lib.opus_encode(encoder, (c.c_int16 * length)(*samples), length, data, 1276)
                if size <= 1: raise SystemExit('Reference failed to encode audio')
                packet = bytes(data[:size])
                if packet[0] >> 3 >= 12: raise SystemExit('Encoder did not select SILK mono')
                history.append(packet)
                count = 1
                if index % 3 == 2:
                    count = 2 if duration == 60 else 3
                    group = history[-count:]
                    if any(p[0] != packet[0] for p in group):
                        count = 1
                    else:
                        combined = b''.join(group)
                        size = lib.repack((c.c_ubyte * len(combined))(*combined),
                                          (c.c_int * count)(*[len(p) for p in group]), count, data)
                        if size <= 0: raise SystemExit('Reference repacketization failed')
                        packet = bytes(data[:size])
                pcm, out = (c.c_int16 * 1920)(), (c.c_uint32 * 4)()
                result = lib.decode(decoder, data, size, pcm, out)
                if result != length * count or not out[0]: raise SystemExit('Reference decode failed')
                print('transition' if out[3] else 'packet', int(reset_next), packet.hex(), out[1], out[2], *pcm[:out[0]])
                reset_next = bool(out[3])
                if reset_next:
                    lib.opus_decoder_destroy(decoder)
                    decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
                    if not decoder or error.value: raise SystemExit('Decoder reset failed')
        finally:
            lib.opus_encoder_destroy(encoder)
            lib.opus_decoder_destroy(decoder)

    print('# Synthetic voiced symbol packets: every LBRR mask, encoded by upstream primitives.')
    for rate in [8, 12, 16]:
        for duration in [10, 20, 40, 60]:
            for mask in range(1 << (1 if duration <= 20 else duration // 20)):
                data = (c.c_ubyte * 1276)()
                size = lib.symbols(rate, duration, mask, data)
                if size <= 1: raise SystemExit('Reference symbol encoding failed')
                decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
                if not decoder or error.value: raise SystemExit('Decoder initialization failed')
                try:
                    pcm, out = (c.c_int16 * 1920)(), (c.c_uint32 * 4)()
                    if lib.decode(decoder, data, size, pcm, out) != 48 * duration or out[3]:
                        raise SystemExit('Reference symbol decode failed')
                    print('symbols', 1, bytes(data[:size]).hex(), out[1], out[2], *pcm[:out[0]])
                finally:
                    lib.opus_decoder_destroy(decoder)
