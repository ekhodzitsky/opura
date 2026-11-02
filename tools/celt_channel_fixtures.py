"""Optional channel-layout extension to the original CELT component oracle.

Invoked by celt_encoder_fixtures.py --channel-packets. Native component bodies
remain unchanged; only the existing baseline's channel orchestration changes.
Public libopus decoding is unchanged. This is not an unchanged native encoder
byte-parity claim, nor a build/test dependency.
"""
import ctypes as c


def adapt_wrapper(wrapper):
    start = wrapper.index('struct baseline {')
    stop = wrapper.index('int decode_packet(', start)
    block = wrapper[start:stop]
    substitutions = [
        ('int rate,C,first,previous', 'int rate,C,coded,first,previous'),
        ('s->rate=rate;s->C=C;', 's->rate=rate;s->C=C;s->coded=C;'),
        ('int N=120<<LM,C=s->C,upsample', 'int N=120<<LM,CC=s->C,C=s->coded,upsample'),
        ('for(int ch=0;ch<C;ch++){\n        OPUS_COPY(in+', 'for(int ch=0;ch<CC;ch++){\n        OPUS_COPY(in+'),
        ('+120,N,C,upsample', '+120,N,CC,upsample'),
        ('tone_detect(in,C,N+120', 'tone_detect(in,CC,N+120'),
        ('transient_analysis(in,N+120,C,', 'transient_analysis(in,N+120,CC,'),
        ('raw_current[1920]={0};for(int ch=0;ch<C;ch++)', 'raw_current[1920]={0};for(int ch=0;ch<CC;ch++)'),
        ('in,freq,C,C,LM', 'in,freq,C,CC,LM'),
        ('in,long_freq,C,C,LM', 'in,long_freq,C,CC,LM'),
        ('int analysis_enabled=(s->detect&2)', 'if(CC==2&&C==1)channel=0;\n    int analysis_enabled=(s->detect&2)'),
        ('    s->previous=s->previous?', '    if(CC==2&&C==1)OPUS_COPY(s->energy+21,s->energy,21);\n    s->previous=s->previous?'),
        ('ch<C?prefilter_history_hash(s->prefilter->in_mem+C*120+', 'ch<CC?prefilter_history_hash(s->prefilter->in_mem+CC*120+'),
        ('ch<C?prefilter_history_hash(s->prefilter->in_mem+ch*120', 'ch<CC?prefilter_history_hash(s->prefilter->in_mem+ch*120'),
    ]
    for old, new in substitutions:
        if old not in block:
            raise ValueError('Baseline channel observation anchor changed: '+old)
        block = block.replace(old, new)
    # Reset clears signal histories but retains the selected wire layout.
    block = block.replace('int rate=s->rate,C=s->C,detect=s->detect;',
                          'int rate=s->rate,C=s->C,coded=s->coded,detect=s->detect;')
    block = block.replace('s->coded=C;s->detect=detect;s->first=1;s->spread=SPREAD_NORMAL;s->average=256;return prefilter_reset(pf);',
                          's->coded=coded;s->detect=detect;s->first=1;s->spread=SPREAD_NORMAL;s->average=256;return prefilter_reset(pf);')
    block += '\nvoid baseline_set_channels(struct baseline *s,int channels){s->coded=channels;}\n'
    return wrapper[:start]+block+wrapper[stop:]


def fnv16(values):
    result = 0xcbf29ce484222325
    for value in values:
        for byte in int(value).to_bytes(2, 'little', signed=True):
            result = ((result ^ byte)*0x100000001b3) & 0xffffffffffffffff
    return result


def emit(lib, revision):
    lib.baseline_set_channels.argtypes = [c.c_void_p, c.c_int]
    lib.baseline_set_channels.restype = None
    print('# Original scalar CELT components with separate API/wire channels:', revision)
    print('# frame RESET RATE LM CODED FLAGS BUDGET HEX; input PCM; state first330 STREAK SPREAD[4] TRIM[2] PREFILTER[5] HASH[8]')
    print('# decoded RATE API COUNT FINAL_RANGE FNV64_LE_PCM; ten unchanged public native decoder histories per frame')
    seed = 0x4348414e
    def random():
        nonlocal seed
        seed ^= (seed << 13) & 0xffffffff
        seed ^= seed >> 17
        seed ^= (seed << 5) & 0xffffffff
        return seed

    rows = 0
    for rate in [8000,12000,16000,24000,48000]:
        for flags in [0,31,47,63]:
            encoder = lib.baseline_create(rate,2,flags)
            if not encoder:
                raise ValueError('Original component owner allocation failed')
            decoders = []
            try:
                for output in [8000,12000,16000,24000,48000]:
                    for api in [1,2]:
                        error = c.c_int()
                        decoder = lib.opus_decoder_create(output,api,c.byref(error))
                        if not decoder or error.value:
                            raise ValueError('Native decoder allocation failed')
                        decoders.append((output,api,decoder))
                for frame in range(32):
                    reset = frame in [0,16]
                    if reset:
                        if lib.baseline_reset(encoder):
                            raise ValueError('Original component reset failed')
                        for _,_,decoder in decoders:
                            if lib.reset_decoder(decoder):
                                raise ValueError('Native decoder reset failed')
                    lm = frame%4
                    n = (120 << lm)*rate//48000
                    coded = [2,2,1,1,1,2,1,2][frame%8]
                    lib.baseline_set_channels(encoder,coded)
                    budget = [32,64,96,128,256,24,25,13][frame%8]
                    # Keep the13-byte prefilter boundary on a mono wire frame.
                    if budget == 13:
                        coded = 1
                        lib.baseline_set_channels(encoder,coded)
                    pcm = []
                    pattern = frame//4
                    for i in range(n):
                        phase = (frame*n+i)%160
                        value = (min(phase,160-phase)-40)*600
                        if pattern == 0:
                            pair = (value,value)
                        elif pattern == 1:
                            pair = (value,-value)
                        elif pattern == 2:
                            pair = (0,value)
                        elif pattern == 3:
                            pair = (value,0)
                        elif pattern == 4:
                            pair = ((random()%65536)-32768,(random()%65536)-32768)
                        elif pattern == 5:
                            pair = (0,0)
                        elif pattern == 6:
                            pair = (32767 if i%40==0 else 0,-32768 if i%19==0 else 0)
                        else:
                            pair = (value,24000 if i==n//2 else 0)
                        pcm.extend(pair)
                    data = (c.c_ubyte*1276)()
                    state = (c.c_int32*468)()
                    length = lib.baseline_encode(encoder,lm,(c.c_int16*len(pcm))(*pcm),budget,data,state)
                    if length != budget+1:
                        raise ValueError(f'Original component encoding failed: {rate,flags,frame,length}')
                    values = list(state)
                    values[0] = c.c_uint32(values[0]).value
                    for index in range(460,468):
                        values[index] = c.c_uint32(values[index]).value
                    print('frame',int(reset),rate,lm,coded,flags,budget,bytes(data[:length]).hex())
                    print('input',*pcm)
                    print('state',*values[:330],values[333],*values[443:449],*values[455:468])
                    for output,api,decoder in decoders:
                        decoded = (c.c_int16*(5760*api))()
                        final = c.c_uint32()
                        count = lib.decode_packet(decoder,data,length,decoded,c.byref(final))
                        if count != (120 << lm)*output//48000 or final.value != values[0]:
                            raise ValueError('Unchanged native channel packet decoding failed')
                        print('decoded',output,api,count,final.value,fnv16(decoded[:count*api]))
                    rows += 1
            finally:
                for _,_,decoder in decoders:
                    lib.opus_decoder_destroy(decoder)
                lib.baseline_destroy(encoder)
    if rows != 640:
        raise ValueError('Incomplete channel corpus')
    print('# complete',rows,'frames',rows*10,'native decoded records')
