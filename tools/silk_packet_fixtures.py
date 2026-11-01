#!/usr/bin/env python3
"""Generate SILK packets and internal/final PCM using pinned fixed-point Opus.

Optional developer oracle: compiles the external reference into a temporary
library. The only instrumentation copies decode_core output after the original
routine returns, optionally copies completed decode_frame output, and observes
each frame's transition-redundancy and stereo-layout decisions;
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
parser.add_argument('--fec', action='store_true', help='Emit FEC and subsequent primary decoding streams')
parser.add_argument('--public-output', action='store_true', help='Emit actual public opus_decode PCM at every output rate')
parser.add_argument('--stereo-output', action='store_true', help='Emit public PCM for encoded/API channel transitions')
args = parser.parse_args()
if sum([args.loss, args.fec, args.public_output, args.stereo_output]) > 1: parser.error('Choose one fixture output mode')
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
static opus_int16 captured[3840];
static int captured_length, captured_rate;
static int transition_seen;
static unsigned layout_flags;
void record_stereo_entry(void) { layout_flags |= 64; }
void record_layout(int channels, int mid_only, int has_side, int previous_mid_only, int collapse, int index) {
    layout_flags |= channels == 1 ? 1 : 2;
    if (mid_only) layout_flags |= 4;
    if (!has_side) layout_flags |= 32;
    if (channels == 2 && !mid_only && previous_mid_only) {
        layout_flags |= 8;
        if (index > 0) layout_flags |= 128;
    }
    if (collapse) layout_flags |= 16;
}
static opus_int16 finished[3840];
static int finished_length;
int reference_decode_frame(silk_decoder_state *, ec_dec *, opus_int16 *, opus_int32 *, int, int, int);
int silk_decode_frame(silk_decoder_state *state, ec_dec *decoder, opus_int16 *pcm,
                      opus_int32 *length, int lost, int coding, int arch) {
    int result = reference_decode_frame(state, decoder, pcm, length, lost, coding, arch);
    if (result == 0) {
        if (finished_length + *length > 3840) abort();
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
    if (captured_length + state->frame_length > 3840) abort();
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
/* Independent stereo LBRR masks, including side-only redundancy and
   primary side restart inside a 60 ms payload. */
int stereo_symbols(int rate, int mid_mask, int side_mask, unsigned char *packet) {
    silk_encoder_state state[2] = {{0}};
    ec_enc ec;
    for (int ch=0; ch<2; ch++) {
        state[ch].fs_kHz = rate;
        state[ch].nb_subfr = 4;
        state[ch].frame_length = rate * 20;
        state[ch].predictLPCOrder = rate == 16 ? 16 : 10;
        state[ch].psNLSF_CB = rate == 16 ? &silk_NLSF_CB_WB : &silk_NLSF_CB_NB_MB;
        state[ch].pitch_lag_low_bits_iCDF = rate == 8 ? silk_uniform4_iCDF : rate == 12 ? silk_uniform6_iCDF : silk_uniform8_iCDF;
        state[ch].pitch_contour_iCDF = rate == 8 ? silk_pitch_contour_NB_iCDF : silk_pitch_contour_iCDF;
    }
    int masks[2] = {mid_mask, side_mask};
    ec_enc_init(&ec, packet + 1, 1275);
    for (int ch=0; ch<2; ch++) {
        for (int i=0; i<3; i++) ec_enc_bit_logp(&ec, ch == 0 || i != 0, 1);
        ec_enc_bit_logp(&ec, masks[ch] != 0, 1);
    }
    for (int ch=0; ch<2; ch++)
        if (masks[ch]) ec_enc_icdf(&ec, masks[ch]-1, silk_LBRR_flags_iCDF_ptr[1], 8);
    for (int redundant=1; redundant>=0; redundant--) {
        for (int i=0; i<3; i++) {
            int present[2] = { !redundant || (mid_mask & (1<<i)),
                              redundant ? (side_mask & (1<<i)) : i != 0 };
            if (present[0]) {
                opus_int8 ix[2][3] = {{1,2,2},{1,2,2}};
                silk_stereo_encode_pred(&ec, ix);
                if (redundant ? !present[1] : i == 0)
                    silk_stereo_encode_mid_only(&ec, redundant ? (i != 1) : 1);
            }
            for (int ch=0; ch<2; ch++) {
                if (!present[ch]) continue;
                int coding = i == 0 ? CODE_INDEPENDENTLY
                    : redundant ? (masks[ch] & (1<<(i-1)) ? CODE_CONDITIONALLY : CODE_INDEPENDENTLY)
                    : ch == 1 && i == 1 ? CODE_INDEPENDENTLY_NO_LTP_SCALING : CODE_CONDITIONALLY;
                SideInfoIndices *indices = redundant ? &state[ch].indices_LBRR[i] : &state[ch].indices;
                memset(indices, 0, sizeof(*indices));
                indices->signalType = TYPE_VOICED;
                indices->GainsIndices[0] = coding == CODE_CONDITIONALLY ? 4 : redundant ? 42 : 10;
                for (int k=1; k<4; k++) indices->GainsIndices[k] = 4;
                indices->NLSFIndices[0] = redundant ? 17 : 0;
                indices->NLSFInterpCoef_Q2 = 2;
                indices->lagIndex = 20;
                indices->Seed = i;
                silk_encode_indices(&state[ch], &ec, i, redundant, coding);
                opus_int8 pulses[320] = {0};
                for (int k=ch; k<rate*20; k+=7) pulses[k] = k%2 ? 1 : -1;
                silk_encode_pulses(&ec, indices->signalType, 0, pulses, rate*20);
            }
        }
    }
    int bytes = (ec_tell(&ec)+7)/8;
    ec_enc_shrink(&ec, bytes);
    ec_enc_done(&ec);
    if (ec_get_error(&ec)) return OPUS_INTERNAL_ERROR;
    packet[0] = (((rate == 8 ? 0 : rate == 12 ? 4 : 8) + 3)<<3) | 4;
    return bytes+1;
}
/* Deliberately over-budget FEC prefix: an earlier internal frame fits,
   but later symbols exceed the original 1275-byte entropy ceiling. */
int budget_packet(unsigned char *packet) {
    unsigned char buffer[16384];
    silk_encoder_state state = {0};
    ec_enc ec;
    state.fs_kHz = 16;
    state.nb_subfr = 4;
    state.predictLPCOrder = 16;
    state.psNLSF_CB = &silk_NLSF_CB_WB;
    ec_enc_init(&ec, buffer, sizeof(buffer));
    for (int i=0; i<4; i++) ec_enc_bit_logp(&ec, 1, 1);
    ec_enc_icdf(&ec, 6, silk_LBRR_flags_iCDF_ptr[1], 8);
    for (int frame=0; frame<3; frame++) {
        SideInfoIndices *indices = &state.indices_LBRR[frame];
        indices->signalType = TYPE_UNVOICED;
        indices->GainsIndices[0] = frame ? 4 : 42;
        for (int i=1; i<4; i++) indices->GainsIndices[i] = 4;
        indices->NLSFIndices[0] = 17;
        indices->NLSFInterpCoef_Q2 = 4;
        silk_encode_indices(&state, &ec, frame, 1, frame ? CODE_CONDITIONALLY : CODE_INDEPENDENTLY);
        ec_enc_icdf(&ec, 8, silk_rate_levels_iCDF[0], 8);
        for (int block=0; block<20; block++) {
            ec_enc_icdf(&ec, 17, silk_pulses_per_block_iCDF[8], 8);
            for (int shift=1; shift<10; shift++) ec_enc_icdf(&ec, 17, silk_pulses_per_block_iCDF[9], 8);
            ec_enc_icdf(&ec, 0, silk_pulses_per_block_iCDF[9] + 1, 8);
        }
        for (int sample=0; sample<320; sample++) {
            for (int bit=0; bit<10; bit++) ec_enc_icdf(&ec, 1, silk_lsb_iCDF, 8);
        }
        unsigned char signs[2] = {silk_sign_iCDF[14], 0};
        for (int sample=0; sample<320; sample++) ec_enc_icdf(&ec, 1, signs, 8);
    }
    if (ec_tell(&ec) <= 1275*8) abort();
    ec_enc_done(&ec);
    if (ec_get_error(&ec)) abort();
    packet[0] = 0x58;
    memcpy(packet + 1, buffer, 1275);
    return 1276;
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
    layout_flags = 0;
    int result = opus_decode(decoder, packet, len, pcm, 5760, 0);
    if (result < 0) return result;
    memcpy(raw, captured, captured_length * sizeof(*raw));
    out[0] = captured_length;
    out[1] = captured_rate;
    if (opus_decoder_ctl(decoder, OPUS_GET_FINAL_RANGE(out + 2)) != OPUS_OK) abort();
    out[3] = transition_seen;
    return result;
 }
int decode_complete(OpusDecoder *decoder, const unsigned char *packet, int len, int frame_samples, int decode_fec,
                    opus_int16 *raw, opus_uint32 *out) {
    opus_int16 pcm[5760];
    captured_length = 0;
    finished_length = 0;
    transition_seen = 0;
    layout_flags = 0;
    int result = opus_decode(decoder, packet, len, pcm, frame_samples, decode_fec);
    if (result < 0) return result;
    memcpy(raw, finished, finished_length * sizeof(*raw));
    out[0] = finished_length;
    out[1] = captured_rate;
    if (opus_decoder_ctl(decoder, OPUS_GET_FINAL_RANGE(out + 2)) != OPUS_OK) abort();
    out[3] = transition_seen;
    return result;
 }
int decode_public(OpusDecoder *decoder, const unsigned char *packet, int len, int frame_samples, int fec,
                  opus_int16 *pcm, opus_uint32 *out) {
    captured_length = 0;
    finished_length = 0;
    transition_seen = 0;
    layout_flags = 0;
    int result = opus_decode(decoder, packet, len, pcm, frame_samples, fec);
    if (result < 0) return result;
    out[0] = result;
    out[1] = captured_rate;
    if (opus_decoder_ctl(decoder, OPUS_GET_FINAL_RANGE(out + 2)) != OPUS_OK) abort();
    out[3] = transition_seen;
    return result;
 }
int configure_stereo(OpusEncoder *encoder, int bandwidth, int force_channels) {
    int ret = configure(encoder, bandwidth, 1);
    ret |= opus_encoder_ctl(encoder, OPUS_SET_BITRATE(30000));
    ret |= opus_encoder_ctl(encoder, OPUS_SET_FORCE_CHANNELS(force_channels));
    return ret;
}
int decode_layout(OpusDecoder *decoder, const unsigned char *packet, int len, int frame_samples, int fec,
                  opus_int16 *pcm, opus_uint32 *out) {
    int result = decode_public(decoder, packet, len, frame_samples, fec, pcm, out);
    out[4] = layout_flags;
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
    api_source = (source / 'silk/dec_API.c').read_text()
    api_source = api_source.replace('#include "main.h"',
        '#include "main.h"\nextern void record_layout(int,int,int,int,int,int);\nextern void record_stereo_entry(void);')
    layout_site = '    channel_state[ 0 ].sPLC.enable_deep_plc = decControl->enable_deep_plc;'
    if api_source.count(layout_site) != 1: raise SystemExit('Unexpected stereo layout site')
    api_source = api_source.replace(layout_site,
        '    record_layout(decControl->nChannelsInternal, decode_only_middle, has_side, psDec->prev_decode_only_middle, stereo_to_mono, channel_state[0].nFramesDecoded);\n' + layout_site)
    entry_site = '        silk_memcpy( &channel_state[ 1 ].resampler_state, &channel_state[ 0 ].resampler_state, sizeof( silk_resampler_state_struct ) );'
    if api_source.count(entry_site) != 1: raise SystemExit('Unexpected stereo resampler entry site')
    api_source = api_source.replace(entry_site, '        record_stereo_entry();\n' + entry_site)
    (path / 'api.c').write_text(api_source)
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                    str(path / 'probe.c'), str(path / 'core.c'), str(path / 'decoder.c'), str(path / 'frame.c'), str(path / 'api.c'),
                    *[str(source / f) for f in files if f not in ['silk/decode_core.c', 'src/opus_decoder.c', 'silk/decode_frame.c', 'silk/dec_API.c']],
                    '-lm', '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    pointer, i16, u8, u32 = c.c_void_p, c.POINTER(c.c_int16), c.POINTER(c.c_ubyte), c.POINTER(c.c_uint32)
    for name, types, result in [
        ('opus_encoder_create', [c.c_int, c.c_int, c.c_int, c.POINTER(c.c_int)], pointer),
        ('opus_decoder_create', [c.c_int, c.c_int, c.POINTER(c.c_int)], pointer),
        ('opus_encoder_destroy', [pointer], None), ('opus_decoder_destroy', [pointer], None),
        ('configure', [pointer, c.c_int, c.c_int], c.c_int),
        ('configure_stereo', [pointer, c.c_int, c.c_int], c.c_int),
        ('stereo_symbols', [c.c_int, c.c_int, c.c_int, u8], c.c_int),
        ('symbols', [c.c_int, c.c_int, c.c_int, u8], c.c_int),
        ('budget_packet', [u8], c.c_int),
        ('opus_encode', [pointer, i16, c.c_int, u8, c.c_int], c.c_int),
        ('decode', [pointer, u8, c.c_int, i16, u32], c.c_int),
        ('decode_complete', [pointer, u8, c.c_int, c.c_int, c.c_int, i16, u32], c.c_int),
        ('decode_public', [pointer, u8, c.c_int, c.c_int, c.c_int, i16, u32], c.c_int),
        ('decode_layout', [pointer, u8, c.c_int, c.c_int, c.c_int, i16, u32], c.c_int),
        ('repack', [u8, c.POINTER(c.c_int), c.c_int, u8], c.c_int),
    ]:
        function = getattr(lib, name)
        function.argtypes, function.restype = types, result
    if args.stereo_output:
        print('# Actual public fixed-point opus_decode mono/stereo PCM and final range:', REVISION)
        print('# KIND RESET OUTPUT_HZ API_CHANNELS INPUT_KHZ DURATION_MS HEX RANGE LAYOUT_FLAGS PCM[duration*output_hz/1000*channels]')
        error = c.c_int()
        for output_khz in [8, 12, 16, 24, 48]:
            for api_channels in [1, 2]:
                for sequence in range(4):
                    encoder = lib.opus_encoder_create(48000, 2, 2048, c.byref(error))
                    if not encoder or error.value: raise SystemExit('Encoder initialization failed')
                    decoder = lib.opus_decoder_create(output_khz * 1000, api_channels, c.byref(error))
                    if not decoder or error.value: raise SystemExit('Decoder initialization failed')
                    phase, other_phase, random, reset_next = 0, 0, 0x4c525354 + sequence, True
                    history = []
                    try:
                        def emit_stereo(kind, packet, duration):
                            global decoder, reset_next
                            data = (c.c_ubyte * len(packet))(*packet) if packet else None
                            pcm, out = (c.c_int16 * 11520)(), (c.c_uint32 * 5)()
                            length = duration * output_khz
                            result = lib.decode_layout(decoder, data, len(packet), length, int(kind == 'fec'), pcm, out)
                            if result != length: raise SystemExit('Stereo public reference decode failed')
                            actual_kind = 'transition' if out[3] else kind
                            print(actual_kind, int(reset_next), output_khz*1000, api_channels, out[1], duration,
                                  packet.hex() if packet else '-', out[2], out[4], *pcm[:result*api_channels])
                            reset_next = bool(out[3])
                            if reset_next:
                                lib.opus_decoder_destroy(decoder)
                                decoder = lib.opus_decoder_create(output_khz * 1000, api_channels, c.byref(error))
                                if not decoder or error.value: raise SystemExit('Decoder reset failed')
                        for index in range(24):
                            band = sequence if sequence < 3 else [0, 1, 2][index // 8]
                            force_channels = 1 if 6 <= index <= 9 or 18 <= index <= 20 else 2
                            if lib.configure_stereo(encoder, 1101 + band, force_channels): raise SystemExit('Stereo encoder configuration failed')
                            duration = [10, 20, 40, 60][index // 6]
                            samples = []
                            for i in range(48 * duration):
                                phase = (phase + 1) % 240
                                other_phase = (other_phase + 1) % 173
                                random ^= (random << 13) & 0xffffffff
                                random ^= random >> 17
                                random ^= (random << 5) & 0xffffffff
                                left = ((phase if phase < 120 else 239 - phase) - 60) * 160 + random % 512 - 256
                                right = ((other_phase if other_phase < 86 else 172 - other_phase) - 43) * 190
                                if index <= 3 or 10 <= index <= 12 or (index in [13, 14] and i < 960): right = left
                                if index == 11: left = right = 0
                                samples.extend([left, right])
                            data = (c.c_ubyte * 1276)()
                            size = lib.opus_encode(encoder, (c.c_int16 * len(samples))(*samples), 48 * duration, data, 1276)
                            if size <= 1: raise SystemExit('Stereo audio encoding failed')
                            packet = bytes(data[:size])
                            if packet[0] >> 3 >= 12: raise SystemExit('Expected SILK mode')
                            history.append(packet)
                            if index in [3, 13, 15, 19]: continue
                            if index in [4, 16]: emit_stereo('loss', b'', 120 if index == 4 else 30)
                            if index in [14, 20]: emit_stereo('fec', packet, duration)
                            emit_stereo('packet', packet, duration)
                        group = history[-2:]
                        if group[0][0] != group[1][0]: raise SystemExit('Expected matching final stereo frame configurations')
                        combined = b''.join(group)
                        size = lib.repack((c.c_ubyte * len(combined))(*combined), (c.c_int * 2)(*[len(p) for p in group]), 2, data)
                        if size <= 0: raise SystemExit('Stereo repacketization failed')
                        emit_stereo('packet', bytes(data[:size]), 120)
                    finally:
                        lib.opus_encoder_destroy(encoder)
                        lib.opus_decoder_destroy(decoder)
        output_khz = 48
        # Exhaust all 64 independent mid/side redundancy masks per rate/API layout.
        for api_channels in [1, 2]:
            for input_khz in [8, 12, 16]:
                for mid_mask in range(8):
                    for side_mask in range(8):
                        decoder = lib.opus_decoder_create(48000, api_channels, c.byref(error))
                        if not decoder or error.value: raise SystemExit('Synthetic decoder initialization failed')
                        reset_next = True
                        try:
                            data = (c.c_ubyte * 1276)()
                            size = lib.stereo_symbols(input_khz, mid_mask, side_mask, data)
                            if size <= 1: raise SystemExit('Synthetic stereo encoding failed')
                            packet = bytes(data[:size])
                            emit_stereo('fec', packet, 60)
                            emit_stereo('packet', packet, 60)
                        finally:
                            lib.opus_decoder_destroy(decoder)
        raise SystemExit(0)
    if args.public_output:
        print('# Actual public fixed-point opus_decode PCM and final range:', REVISION)
        print('# KIND RESET OUTPUT_HZ INPUT_KHZ DURATION_MS HEX FINAL_RANGE PCM[duration_ms*output_hz/1000]')
        error = c.c_int()
        for output_khz in [8, 12, 16, 24, 48]:
            for sequence in range(4):
                encoder = lib.opus_encoder_create(48000, 1, 2048, c.byref(error))
                if not encoder or error.value: raise SystemExit('Encoder initialization failed')
                decoder = lib.opus_decoder_create(output_khz * 1000, 1, c.byref(error))
                if not decoder or error.value: raise SystemExit('Decoder initialization failed')
                phase, random, reset_next = 0, 0x50434d21 + sequence, True
                history = []
                try:
                    def emit(kind, packet, duration):
                        global decoder, reset_next
                        data = (c.c_ubyte * len(packet))(*packet) if packet else None
                        pcm, out = (c.c_int16 * 5760)(), (c.c_uint32 * 4)()
                        length = duration * output_khz
                        result = lib.decode_public(decoder, data, len(packet), length, int(kind == 'fec'), pcm, out)
                        if result != length: raise SystemExit('Public reference decode failed')
                        actual_kind = 'transition' if out[3] else kind
                        print(actual_kind, int(reset_next), output_khz * 1000, out[1], duration,
                              packet.hex() if packet else '-', out[2], *pcm[:result])
                        reset_next = bool(out[3])
                        if reset_next:
                            lib.opus_decoder_destroy(decoder)
                            decoder = lib.opus_decoder_create(output_khz * 1000, 1, c.byref(error))
                            if not decoder or error.value: raise SystemExit('Decoder reset failed')
                    for index in range(12):
                        band = sequence if sequence < 3 else [0, 1, 2][index // 4]
                        if lib.configure(encoder, 1101 + band, int(sequence != 0)):
                            raise SystemExit('Encoder configuration failed')
                        duration = [10, 20, 40, 60][index // 3]
                        samples = []
                        for _ in range(48 * duration):
                            phase = (phase + 1) % 240
                            random ^= (random << 13) & 0xffffffff
                            random ^= random >> 17
                            random ^= (random << 5) & 0xffffffff
                            value = ((phase if phase < 120 else 239 - phase) - 60) * 160
                            samples.append(0 if 6 <= index <= 8 else value + random % 1024 - 512)
                        data = (c.c_ubyte * 1276)()
                        size = lib.opus_encode(encoder, (c.c_int16 * len(samples))(*samples), len(samples), data, 1276)
                        if size <= 1: raise SystemExit('Reference audio encoding failed')
                        packet = bytes(data[:size])
                        if packet[0] >> 3 >= 12: raise SystemExit('Expected SILK mono')
                        history.append(packet)
                        if index % 3 == 1: continue
                        if index in [2, 8]: emit('loss', b'', 120 if index == 2 else 30)
                        if index in [5, 11]: emit('fec', packet, duration)
                        emit('packet', packet, duration)
                    group = history[-2:]
                    if group[0][0] != group[1][0]: raise SystemExit('Expected matching final frame configurations')
                    combined = b''.join(group)
                    size = lib.repack((c.c_ubyte * len(combined))(*combined),
                                      (c.c_int * 2)(*[len(p) for p in group]), 2, data)
                    if size <= 0: raise SystemExit('Reference repacketization failed')
                    emit('packet', bytes(data[:size]), 120)
                finally:
                    lib.opus_encoder_destroy(encoder)
                    lib.opus_decoder_destroy(decoder)
        raise SystemExit(0)
    if args.fec:
        print('# Pinned Opus v1.6.1 mono FEC and subsequent primary decode_frame PCM:', REVISION)
        print('# KIND RESET HEX RATE_KHZ FINAL_RANGE PCM[frame_duration_ms*rate]')
        error = c.c_int()
        for sequence in range(7):
            encoder = lib.opus_encoder_create(48000, 1, 2048, c.byref(error))
            if not encoder or error.value: raise SystemExit('Encoder initialization failed')
            decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
            if not decoder or error.value: raise SystemExit('Decoder initialization failed')
            phase, random, reset_next = 0, 0x66656321 + sequence, True
            try:
                for index in range(32):
                    band = sequence // 2 if sequence < 6 else [0, 1, 2, 1][index // 8]
                    if lib.configure(encoder, 1101 + band, sequence % 2 if sequence < 6 else 1):
                        raise SystemExit('Encoder configuration failed')
                    duration = [10, 20, 40, 60][index // 8]
                    samples = []
                    for _ in range(48 * duration):
                        phase = (phase + 1) % 240
                        random ^= (random << 13) & 0xffffffff
                        random ^= random >> 17
                        random ^= (random << 5) & 0xffffffff
                        value = ((phase if phase < 120 else 239 - phase) - 60) * 160
                        samples.append(0 if index % 16 >= 12 else value + random % 1024 - 512)
                    data = (c.c_ubyte * 1276)()
                    size = lib.opus_encode(encoder, (c.c_int16 * len(samples))(*samples), len(samples), data, 1276)
                    if size <= 1: raise SystemExit('Reference audio encoding failed')
                    packet = bytes(data[:size])
                    if packet[0] >> 3 >= 12: raise SystemExit('Expected SILK mono')
                    # Omit one encoded packet, then recover its interval using
                    # FEC from the next packet before decoding current audio.
                    if index % 4 == 1: continue
                    modes = [1, 0] if index % 4 == 2 or index == 0 else [0]
                    for fec in modes:
                        pcm, out = (c.c_int16 * 1920)(), (c.c_uint32 * 4)()
                        if lib.decode_complete(decoder, data, size, 48 * duration, fec, pcm, out) != 48 * duration or not out[0]:
                            raise SystemExit('Reference FEC decode failed')
                        kind = 'transition' if out[3] else 'fec' if fec else 'packet'
                        print(kind, int(reset_next), packet.hex(), out[1], out[2], *pcm[:out[0]])
                        reset_next = bool(out[3])
                        if reset_next:
                            lib.opus_decoder_destroy(decoder)
                            decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
                            if not decoder or error.value: raise SystemExit('Decoder reset failed')
            finally:
                lib.opus_encoder_destroy(encoder)
                lib.opus_decoder_destroy(decoder)
        print('# Synthetic voiced symbol packets: FEC then primary for every LBRR mask.')
        for rate in [8, 12, 16]:
            for duration in [10, 20, 40, 60]:
                for mask in range(1 << (1 if duration <= 20 else duration // 20)):
                    data = (c.c_ubyte * 1276)()
                    size = lib.symbols(rate, duration, mask, data)
                    if size <= 1: raise SystemExit('Reference symbol encoding failed')
                    decoder = lib.opus_decoder_create(48000, 1, c.byref(error))
                    if not decoder or error.value: raise SystemExit('Decoder initialization failed')
                    try:
                        for fec in [1, 0]:
                            pcm, out = (c.c_int16 * 1920)(), (c.c_uint32 * 4)()
                            if lib.decode_complete(decoder, data, size, 48 * duration, fec, pcm, out) != 48 * duration or out[3]:
                                raise SystemExit('Reference symbol FEC decode failed')
                            print('fec' if fec else 'packet', fec, bytes(data[:size]).hex(), out[1], out[2], *pcm[:out[0]])
                    finally:
                        lib.opus_decoder_destroy(decoder)
        data = (c.c_ubyte * 1276)()
        size = lib.budget_packet(data)
        print('# Deliberately over-budget/truncated LBRR prefix; atomic resource-error regression.')
        print('budget', 1, bytes(data[:size]).hex(), 16, 0)
        raise SystemExit(0)
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
                                                 48 * duration, 0, pcm, out)
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
