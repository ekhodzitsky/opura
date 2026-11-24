# Third-party notices

## Opus entropy coding attribution

The Rust range decoder and encoder follow the algorithms and integer arithmetic
of RFC 6716 and the Xiph.Org Opus entropy coder (entdec.c, entenc.c and entcode.c).
Classic Laplace coding additionally follows celt/laplace.c under the CELT notices
below. Optional reference benchmark/fixture tools compile external original source
only; it is not a Cargo dependency.
Reference revision: v1.6.1, commit 22244de5a79bd1d6d623c32e72bf1954b56235be.
No C implementation is linked or included in the library. The following
notices and terms are retained for the adapted entropy-coding implementation.

```text
/* Copyright (c) 2001-2011 Timothy B. Terriberry
   Copyright (c) 2008-2009 Xiph.Org Foundation */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

## SILK attribution

The header ordering and LBRR probability tables follow RFC 6716 and Opus
`silk/dec_API.c`, `silk/decode_indices.c`, `silk/tables_other.c`,
`silk/tables_gain.c`, `silk/gain_quant.c` and `silk/log2lin.c` at the revision
above. The gain reconstruction and probability tables are adapted under the
same notices below. NLSF codebooks, reconstruction and stabilization are adapted
from `silk/tables_NLSF_CB_NB_MB.c`, `silk/tables_NLSF_CB_WB.c`,
`silk/NLSF_unpack.c`, `silk/NLSF_decode.c`, `silk/NLSF_stabilize.c` and
`silk/decode_parameters.c`, under the same notices. LPC conversion and its
cosine table follow `silk/NLSF2A.c`, `silk/table_LSF_cos.c`, `silk/LPC_fit.c`,
`silk/LPC_inv_pred_gain.c`, `silk/bwexpander_32.c` and `silk/Inlines.h`.
Stereo predictor decoding and buffered mid/side reconstruction follow
`silk/stereo_decode_pred.c`, `silk/stereo_MS_to_LR.c` and stereo tables
in `silk/tables_other.c`.
Packet channel/LBRR ordering, side restart, mono/stereo transitions and output
delay buffering follow `silk/dec_API.c`.
Decoder output resampling and coefficient tables additionally follow
`silk/resampler.c`, `silk/resampler_private_up2_HQ.c`,
`silk/resampler_private_IIR_FIR.c`, `silk/resampler_private_down_FIR.c`,
`silk/resampler_private_AR2.c` and `silk/resampler_rom.c` / `.h`.
Packet-loss concealment and recovery follow `silk/PLC.c`, `silk/PLC.h`,
`silk/sum_sqr_shift.c`, `silk/bwexpander.c` and the loss paths in
`silk/decode_frame.c` / `silk/decode_core.c`.
Comfort-noise estimation and filtering additionally follow `silk/CNG.c`
and its fixed-point square-root approximation in `silk/Inlines.h`.
Pitch/LTP reconstruction and tables additionally follow `silk/decode_pitch.c`,
`silk/tables_pitch_lag.c`, `silk/pitch_est_tables.c` and `silk/tables_LTP.c`.
Pulse/excitation decoding additionally follows `silk/decode_pulses.c`,
`silk/shell_coder.c`, `silk/code_signs.c`, `silk/tables_pulses_per_block.c`,
`silk/decode_core.c` and the PRNG constants in `silk/SigProc_FIX.h`.
Loss-free synthesis also adapts `silk/LPC_analysis_filter.c`, the history update
in `silk/decode_frame.c`, and fixed-point gain refinement in `silk/Inlines.h`.

```text
/***********************************************************************
Copyright (c) 2006-2011, Skype Limited. All rights reserved.
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
- Redistributions of source code must retain the above copyright notice,
this list of conditions and the following disclaimer.
- Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.
- Neither the name of Internet Society, IETF or IETF Trust, nor the
names of specific contributors, may be used to endorse or promote
products derived from this software without specific prior written
permission.
THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.
***********************************************************************/
```

## CELT scalar math attribution

The fixed-point scalar algorithms and constants follow `celt/mathops.c`,
`celt/mathops.h` and `celt/fixed_generic.h` at the pinned revision above.
No C implementation is linked or vendored into the Rust library.

```text
/* Copyright (c) 2002-2008 Jean-Marc Valin
   Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2009 Xiph.Org Foundation
   Copyright (c) 2024 Arm Limited
   Written by Jean-Marc Valin, and Yunho Huh */
/**
   @file mathops.h
   @brief Various math functions
*/
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

## CELT energy and Laplace attribution

The energy probability tables, coarse/fine/final algorithms and Laplace symbols
follow `celt/quant_bands.c` and `celt/laplace.c` at the pinned revision above.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2009 Xiph.Org Foundation
   Written by Jean-Marc Valin */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

## CELT CWRS pulse-vector attribution

The symmetric U/V recurrence and signed codebook ordering follow `celt/cwrs.c`
at the pinned revision above. Compile-time tables are derived from the recurrence.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2009 Xiph.Org Foundation
   Copyright (c) 2007-2009 Timothy B. Terriberry
   Written by Timothy B. Terriberry and Jean-Marc Valin */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

## CELT PVQ normalization and spreading attribution

Normalized pulse reconstruction, exponential rotations and collapse masks follow
`celt/vq.c` at the pinned revision above. High-precision reciprocal-root refinement
additionally follows `celt/mathops.c` under its retained scalar-math notices.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2009 Xiph.Org Foundation
   Written by Jean-Marc Valin */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

## CELT FFT attribution

The mixed-radix butterflies, stage scaling and standard plan/twiddle data follow
`celt/kiss_fft.c`, `celt/_kiss_fft_guts.h` and `celt/static_modes_fixed.h` at the
pinned revision above. [Upstream FFT source](https://github.com/xiph/opus/blob/22244de5a79bd1d6d623c32e72bf1954b56235be/celt/kiss_fft.c)

```text
/*Copyright (c) 2003-2004, Mark Borgerding
  Lots of modifications by Jean-Marc Valin
  Copyright (c) 2005-2007, Xiph.Org Foundation
  Copyright (c) 2008,      Xiph.Org Foundation, CSIRO

  All rights reserved.

  Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions are met:

    * Redistributions of source code must retain the above copyright notice,
       this list of conditions and the following disclaimer.
    * Redistributions in binary form must reproduce the above copyright notice,
       this list of conditions and the following disclaimer in the
       documentation and/or other materials provided with the distribution.

  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
  AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
  IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
  ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
  LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
  CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
  SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
  INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
  CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
  ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
  POSSIBILITY OF SUCH DAMAGE.*/

/* This code is originally from Mark Borgerding's KISS-FFT but has been
   heavily modified to better suit Opus */
```

## CELT forward/inverse MDCT attribution

Forward window/folding, scaling, rotations/headroom control, inverse rotations,
TDAC synthesis and the standard trig/window
tables follow `celt/mdct.c` and `celt/static_modes_fixed.h` at the pinned revision.
The scalar FFT retains the separate KISS FFT notices above.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2008 Xiph.Org Foundation
   Written by Jean-Marc Valin */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/

/* This is a simple MDCT implementation that uses a N/4 complex FFT
   to do most of the work. It should be relatively straightforward to
   plug in pretty much and FFT here.

   This replaces the Vorbis FFT (and uses the exact same API), which
   was a bit too messy and that was ending up duplicating code
   (might as well use the same FFT everywhere).

   The algorithm is similar to (and inspired from) Fabrice Bellard's
   MDCT implementation in FFMPEG, but has differences in signs, ordering
   and scaling in many places.
*/
```

## CELT spectral scaling and de-emphasis attribution

Spectral denormalization and mean energies follow `celt/bands.c` and
`celt/quant_bands.c`. Band edges and pre-emphasis coefficients follow
`celt/modes.c` / `celt/static_modes_fixed.h`. Output feedback, clipping and
decimation follow `celt/celt_decoder.c`, with scalar arithmetic in `celt/arch.h`.
All refer to the pinned revision above and retain its notices here.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2009 Xiph.Org Foundation
   Copyright (c) 2008-2009 Gregory Maxwell
   Written by Jean-Marc Valin and Gregory Maxwell */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

Additional original decoder notice retained for de-emphasis:

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2010 Xiph.Org Foundation
   Copyright (c) 2008 Gregory Maxwell
   Written by Jean-Marc Valin and Gregory Maxwell */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

## CELT comb postfilter attribution

Constant/windowed comb filtering and tap gains follow `celt/celt.c`; normal frame
parameter scheduling follows `celt/celt_decoder.c` at the pinned revision above.
The standard window uses the retained inverse-MDCT table notices.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2010 Xiph.Org Foundation
   Copyright (c) 2008 Gregory Maxwell
   Written by Jean-Marc Valin and Gregory Maxwell */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```

## CELT synthesis backend ordering

Channel projection, transient block scheduling and synthesis clipping follow
`celt/celt_decoder.c` at the pinned revision above, under its retained original
CSIRO/Xiph.Org/Gregory Maxwell notice. The composed scalar, CWRS/PVQ, transform,
band scaling, postfilter and de-emphasis stages retain their notices above.

## CELT control and TF attribution

Prefix/silence accounting, postfilter/transient/intra guards and TF/select/spread
decoding follow `celt/celt_decoder.c`; TF mappings follow `celt/celt.c` and
probability models follow `celt/celt.h` at the pinned revision. Their original
CSIRO/Xiph.Org/Gregory Maxwell notices and redistribution terms are retained above.

## CELT bit allocation attribution

Interpolation, skip/stereo reservation, fine/PVQ distribution and fractional-log
tables follow `celt/rate.c`. Dynamic boosts/trim/anti-collapse guards follow
`celt/celt_decoder.c`; caps follow `celt/celt.c` and standard vectors/logN/cache
tables follow `celt/modes.c` / `celt/static_modes_fixed.h` at the pinned revision.
Their additional original decoder/mode notices are retained above.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2009 Xiph.Org Foundation
   Written by Jean-Marc Valin */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```


## CELT band arithmetic attribution

Coded theta/log2tan, Haar, Hadamard ordering and stereo merge follow
`celt/bands.c`; folded/noise normalization follows `celt/vq.c` at the pinned
revision above. Their original CSIRO/Xiph.Org/Gregory Maxwell notices and
redistribution terms are retained above. The optional oracle extracts these
functions unchanged into temporary developer probes; no C is linked into Cargo.


## CELT recursive partition attribution

Theta resolution/entropy, recursive partition/gain/bit balancing and noise/folding
follow `celt/bands.c`; pulse mapping follows `celt/rate.h` and standard cache
indices/bits follow `celt/static_modes_fixed.h` at the pinned revision above.
Their original CSIRO/Xiph.Org/Gregory Maxwell notices and terms are retained above.


## CELT mono/stereo band orchestration attribution

TF/Haar/Hadamard scheduling, folding output, one-coordinate signs and stereo
angle/intensity/inversion/N2/merge scheduling follow `celt/bands.c` at the pinned
revision above. Original CSIRO/Xiph.Org/Gregory Maxwell notices and redistribution
terms are retained above; existing scalar/PVQ/transform notices also apply.


## CELT all-band spectral assembly attribution

Cross-band balance, folding selection/masks, special Hybrid folding and
stereo-mode transitions follow `quant_all_bands` / `special_hybrid_folding` in
`celt/bands.c` at the pinned revision above, under the retained original
CSIRO/Xiph.Org/Gregory Maxwell notices and redistribution terms.


## CELT transient anti-collapse attribution

Bit-depth/energy attenuation, mono history merging, short-block noise and
renormalization scheduling follow `anti_collapse` in `celt/bands.c` at the pinned
revision above, under the retained CSIRO/Xiph.Org/Gregory Maxwell notices and
redistribution terms. Existing scalar math and vector normalization notices apply.


## Normal CELT decoder integration attribution

Normal-frame stage ordering, channel/energy/history/RNG and silence policy follow
`celt/celt_decoder.c`; bandwidth/final-range packet behavior follows
`src/opus_decoder.c` at the pinned revision above. Existing original retained
CSIRO/Xiph.Org/Gregory Maxwell decoder notices and component terms apply.
The optional complete-packet oracle appends read-only snapshot functions to
original external decoder files, preserving all original arithmetic statements
and notices. No reference C implementation is linked into Cargo.


## CELT autocorrelation, LPC and filters attribution

Autocorrelation/window/scaling, Levinson-Durbin/Q12 fitting, FIR and canonical
IIR follow `celt/celt_lpc.c` at the pinned revision above. Scalar math and standard
window attribution also apply. The original additional notice is retained here.

```text
/* Copyright (c) 2009-2010 Xiph.Org Foundation
   Written by Jean-Marc Valin */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```


## CELT PLC pitch analysis attribution

Classic downsampling/whitening, FIR5, coarse/fine correlations, normalized pitch
ranking and pseudo interpolation follow `celt/pitch.c`; fixed PLC search geometry
follows `celt/celt_decoder.c` at the pinned revision above. Their original
CSIRO/Xiph.Org/Jean-Marc Valin notices and terms are retained above; existing
LPC and scalar math attribution also apply.


## CELT concealment and recovery attribution

Periodic/noise PLC selection, LPC stabilization, excitation/synthesis energy
checks, noise/background decay, prefilter/TDAC, loss counters and recovery follow
`celt/celt_decoder.c`; public loss/DTX scheduling and initial silence follow
`src/opus_decoder.c` at the pinned revision above. Their existing original
CSIRO/Xiph.Org/Gregory Maxwell notices and terms are retained above, along with
LPC, pitch, scalar math, comb and MDCT component notices. Optional guard oracles
extract unchanged original safety blocks into temporary probes only.


## Hybrid packet integration attribution

Shared SILK/CELT entropy ordering, Hybrid redundancy-presence parsing, start-band
selection and fixed-point PCM accumulation follow `src/opus_decoder.c` and
`celt/arch.h` at the pinned revision above. SILK/CELT component attribution
continues to apply. The packet integration source carries the following notice:

```c
/* Copyright (c) 2010 Xiph.Org Foundation, Skype Limited
   Copyright (c) 2024 Arm Limited
   Written by Jean-Marc Valin and Koen Vos */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```


Hybrid loss/FEC scheduling additionally follows `src/opus_decoder.c`; short
SILK-frame advancement and LBRR gaps follow `silk/dec_API.c`. The original
signed16 `SMULBB` LTP normalization behavior follows `silk/PLC.c`; the extended
positive signed16 comfort-noise scale context follows `silk/CNG.c`. Their
original copyright notices and redistribution terms are retained above.


## Unified Opus decoder attribution

All-mode packet/loss/FEC orchestration, primary entropy-storage shrinking,
redundant CELT-tail ordering, mode resets, squared-window PCM fades and final
range XOR follow `src/opus_decoder.c`; storage behavior follows `celt/entdec.c`
and window/fixed-point arithmetic follows the CELT sources identified above.
The original Xiph.Org Foundation/Skype Limited/Arm Limited packet integration
notice and all SILK/CELT component notices and terms are retained above.


## Optional official conformance validation

The RFC8251 vector archive is an external validation input, verified against
its official file hashes and a pinned archive SHA256. It is not redistributed
in the repository or fetched by Cargo. The optional runner compiles the
unchanged external `src/opus_compare.c` at the pinned Opus revision; its original
Xiph.Org Foundation/Mozilla Corporation notice and BSD terms remain in that
external source. No comparator or reference codec is linked into Opura.


## CELT input pre-emphasis attribution

Signed16 input scaling, zero insertion and per-channel pre-emphasis follow
`celt/celt_encoder.c::celt_preemphasis` at the pinned revision above. The standard
coefficient retains the mode-table attribution above. This stage supports the
classic fixed-point signed16 domain, without QEXT, RES24 or custom modes.

```text
/* Copyright (c) 2007-2008 CSIRO
   Copyright (c) 2007-2010 Xiph.Org Foundation
   Copyright (c) 2008 Gregory Maxwell
   Written by Jean-Marc Valin and Gregory Maxwell */
/*
   Redistribution and use in source and binary forms, with or without
   modification, are permitted provided that the following conditions
   are met:

   - Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   - Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
   ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/
```


## CELT channel/subframe analysis attribution

Long/short MDCT orchestration, stereo-to-mono rounding and low-rate spectral
scaling/cutoff follow `celt/celt_encoder.c::compute_mdcts` at the pinned revision.
The CELT encoder notices and terms retained in the preceding input section apply;
individual transforms retain the forward/inverse MDCT and KISS FFT notices above.


## CELT encoder band analysis attribution

Band amplitudes and normalization follow `celt/bands.c::compute_band_energies`
and `normalise_bands`; mean-adjusted log energies follow
`celt/quant_bands.c::amp2Log2`. Refined square root and normalized reciprocal
follow `celt/mathops.c::celt_sqrt32`, `celt_rcp_norm16` and `celt_rcp_norm32`.
All use the pinned revision above, with their CELT band/scalar notices and terms
already retained here. Canonical band edges, log widths and means keep their
existing mode/table attribution. Active-domain overflow validation is added in Rust.


## CELT encoder energy quantization attribution

Coarse temporal/inter-band prediction, decay/scarcity/LFE policy, inter/intra
RDO and fine/final refinement follow `celt/quant_bands.c` at the pinned revision.
The CELT energy/probability/quant_bands notices and terms already retained above
apply. Rust adds atomic stage failures, checked physical-target arithmetic and
deterministic zero residuals outside each newly selected coarse layout; original
unused per-frame scratch is not treated as initialized state by optional probes.


## CELT CWRS encoding attribution

Vector ranking and uniform pulse encoding follow `celt/cwrs.c::icwrs` and
`encode_pulses` at the pinned revision above. Existing CWRS notices and terms
apply. Optional encoder fixtures use the unchanged original SMALL_FOOTPRINT
recurrence so every supported representable geometry is covered, including
geometries outside the optimized standard-mode table. Zero-pulse encoding is
a no-bit extension paired with Opura's existing zero-pulse decoding contract.


## CELT PVQ pulse search and encoding attribution

Forward spreading, adaptive-headroom pyramid/greedy search, strict score ties,
silence fallback, sign restoration and resynthesis follow `celt/vq.c::alg_quant`,
`op_pvq_search_c`, `exp_rotation` and `normalise_residual` at the pinned revision.
Existing normalized-band/vq notices and terms apply. Rust validates a bounded
normalized Q24 input domain, retains immutable caller input, and stages returned
reconstruction before the single atomic CWRS write. QEXT refinement is excluded.


## CELT recursive mono encoding attribution

Measured energy-ratio angles and recursive mono encoding follow
`celt/vq.c::stereo_itheta` (mono path) and `celt/bands.c::compute_theta` /
`quant_partition` at the pinned revision above. Normalized arctangent functions
follow `celt/mathops.h::celt_atan_norm` and `celt_atan2p_norm`. Existing band/vq/
scalar notices and terms apply. Rust shares unchanged zero-pulse reconstruction
with decoding, validates bounded immutable input and commits complete operations
atomically. Stereo/TF band encoding is not supplied by this mono primitive.


## CELT stereo and time-frequency band encoding attribution

Stereo energy-ratio measurement, amplitude weighting, mid/side transforms,
angle candidate/model/inversion policy, signs and TF band orchestration follow
`celt/vq.c::stereo_itheta` and `celt/bands.c::intensity_stereo`, `stereo_split`,
`compute_theta`, `quant_band_n1`, `quant_band` and `quant_band_stereo` at the pinned
revision. Existing CELT band/vq/scalar notices and terms apply. Rust shares band
validation and TF reconstruction between entropy directions, validates bounded
immutable normalized inputs and commits complete operations atomically.


## CELT complete-spectrum encoding and theta RDO attribution

Full-band budget/folding/channel orchestration and two-candidate stereo angle
selection follow `celt/bands.c::quant_all_bands` and `compute_channel_weights` at
the pinned revision. Existing CELT bands/vq/scalar notices and terms apply.
Rust shares a statically dispatched spectral driver between entropy directions,
validates bounded immutable input/amplitude domains and stages full writer/output.
Optional original evidence enables RESYNTH and uses read-only entropy-symbol
observers around prefix decoding; original codec function bodies stay unchanged.


## CELT TF and spreading control encoding attribution

Differential/reserved-select TF writing follows `celt/celt_encoder.c::tf_encode`
at the pinned revision. Spreading-field coding follows its spread ICDF budget
check; existing CELT encoder/control/table notices and terms apply. Rust accepts
immutable binary decisions and returns effective resolutions, validates frame/
writer domains and stages the whole operation. TF/spread analysis is separate.


## CELT allocation encoding attribution

Dynamic boost and trim signalling follow `celt/celt_encoder.c`; retained-band
hysteresis and stereo decisions follow `celt/rate.c::interp_bits2pulses` /
`clt_compute_allocation` at the pinned revision. Existing CELT encoder/rate/table
notices and terms apply. Rust shares mandatory numerical allocation math with
decoding, validates immutable requested policies and stages whole writer commits.
Optional original probes reuse a deterministic project-owned entropy prefix helper;
no reference codec code is linked into ordinary builds/tests.


## Stateful CELT PCM packet encoder attribution

PCM/frame/energy/control/allocation/spectral composition, stable-energy residual
feedback and frame-history updates follow `celt/celt_encoder.c` at the pinned
revision. Existing CELT encoder/component notices and terms apply. Rust adds
fixed-storage complete Opus packet output, immutable input, typed geometry/budget
errors and atomic whole-stream commits. Initial explicit all-long control
policies do not implement the reference encoder's adaptive signal analysis.
Optional evidence includes original `celt_encoder.c` to call private component
helpers, links the unchanged original source externally and appends read-only
decoder-state observers. Unused inactive residual scratch is canonicalized
without changing any selected-band coding or decoded signal.


## CELT tone/transient analysis attribution

`src/celt/analysis/transient.rs` and its constants follow `tone_detect`,
`tone_lpc`, `transient_analysis`, `acos_approx` and their inverse table in
`celt/celt_encoder.c` at the pinned revision. Existing CELT encoder notices and
terms apply. Private wide-Q29 division follows `celt/mathops.c::frac_div32_q29`;
its public fractional-domain contract remains unchanged. Rust bounds immutable
physical input and uses fixed safe storage. Optional packet selection and the
anti-collapse streak follow original encoder control flow. The external metric
observer cross-checks all ordinary outputs against unchanged original analysis.


## CELT time/frequency analysis attribution

`src/celt/analysis/tf.rs` follows `celt/celt_encoder.c::l1_metric` and
`tf_analysis` at the pinned revision, reusing original `haar1` arithmetic and
`tf_select_table` through the existing Rust stages/tables. Existing CELT encoder,
band and control notices and terms apply. Rust adds bounded immutable normalized
input, fixed storage and typed validation. Packet integration follows native
budget/tone gating and switching cost with explicit neutral importance. The
optional read-only metric observer cross-checks ordinary decisions/select
against unchanged original analysis; native code is never a Cargo dependency.


## CELT perceptual dynamic-allocation analysis attribution

`src/celt/analysis/dynalloc.rs` and canonical constants follow
`celt/celt_encoder.c::dynalloc_analysis`, `median_of_3` and `median_of_5` at the
pinned revision. Existing CELT encoder/rate/math/table notices and terms apply.
Importance uses the existing classic exponent path selected by the original
non-QEXT build. Rust bounds immutable logs/controls, retains exact raw cap-hit
requests, and supplies counts clamped to 0..255 with a standard-cap wire proof.
Optional packet integration follows native transient long-window comparison,
energy-history ordering and perceptual TF weighting under explicit CBR controls.
All reference code remains an optional external validation tool.


## CELT pulse spreading analysis attribution

`src/celt/analysis/spreading.rs` follows `celt/bands.c::spreading_decision` at
the pinned revision. Existing CELT band/encoder notices and terms apply. Native
initial histories are observed through unchanged encoder initialization. Rust
bounds immutable normalized inputs/weights and checked histories, committing
all state atomically. Optional packet selection follows the original spreading
budget/short-window/byte gates and fallback history, using native masking weights.
Reference algorithms remain optional external validation and never enter Cargo.


## CELT trim and stereo allocation analysis attribution

`src/celt/analysis/allocation_policy.rs` and its canonical tables follow
`celt/celt_encoder.c::stereo_analysis`, `alloc_trim_analysis`, equivalent-rate
arithmetic and intensity arrays; `celt/bands.c::hysteresis_decision`; and
`celt/vq.c::celt_inner_prod_norm_shift` at the pinned revision. Existing CELT
encoder/band/math/table notices and terms apply. Rust bounds immutable inputs,
checks the final tilt accumulator and stages saving history atomically. Optional
read-only cost/trim observers cross-check ordinary results against unchanged
original functions. No reference code enters normal Cargo builds or tests.

Optional CELT packet allocation policy integration follows the original
post-dynamic trim availability guard, desired intensity hysteresis and LM0 M/S
policy in celt_encoder.c. The fixed Rust rate engine remains shared with decoding;
external original component assembly and artificial entropy-tail probes are
validation tools only. Existing encoder/allocation notices and terms apply.


## CELT encoder pitch analysis attribution

`src/celt/pitch/encoder.rs` follows unchanged `celt/pitch.c::remove_doubling`
and `compute_pitch_gain`; shared private downsampling/whitening/search geometry
additionally follows `celt/celt_encoder.c::run_prefilter` at the pinned revision.
Existing CSIRO/Xiph.Org/Jean-Marc Valin pitch, LPC and scalar-math notices and
terms apply. Rust checks geometry/history/signal and accumulator domains with
immutable inputs and fixed storage. Original scalar/context observers remain
optional external validation; no reference implementation enters Cargo.


## CELT encoder prefilter attribution

`src/celt/prefilter.rs` follows `celt/celt_encoder.c::run_prefilter` and
`celt/celt.c::comb_filter`/`comb_filter_const_c` at the pinned revision. Existing
CSIRO/Xiph.Org/Jean-Marc Valin pitch, comb, window and scalar-math notices/terms
apply. Only immutable tap/window data is shared with the decoder; encoder lag
reads use separate raw source storage. Original signed16 stereo thresholds,
rounding, bias, saturation and history order are preserved. External unchanged
original prefilter/constructor/reset observers are optional validation only and
never enter Cargo builds or tests.


CELT PCM prefilter integration follows original raw-analysis/filter/MDCT order,
byte/resource gates, cached-tell comb period/gain/tap signaling and actual
active-long-filter spreading HF updates in celt_encoder.c. Existing encoder,
prefilter, comb, spreading and control-table notices and terms apply. Optional
comb-field evidence extracts the original signaling block unchanged; packet
assembly owns unchanged original prefilter states and read-only history hashes.
Reference code remains external validation and never enters Cargo.

## Audio command container and fixture provenance

The native Ogg command follows [RFC 3533](https://www.rfc-editor.org/rfc/rfc3533.html)
and [RFC 7845](https://www.rfc-editor.org/rfc/rfc7845.html), with original Rust
page/CRC and RIFF/WAVE I/O code. The three small `tests/audio` streams encode
synthetic integer waveforms generated by `tools/audio_cli_fixtures.py` using
unchanged external Opus v1.6.1 at the reference revision above. Their paired
PCM is the original public fixed-point decoder output after pre-skip and tail
trimming. The manifest records geometry and mode. No third-party recording
or codec source is embedded in these fixtures. Regeneration is optional;
normal builds and tests use only Rust and the stored data.

## SILK encoder attribution

Quantized-frame serialization additionally follows unchanged Opus v1.6.1
`silk/encode_indices.c`, `silk/encode_pulses.c`, `silk/shell_coder.c` and
`silk/code_signs.c`, covered by the retained Skype SILK terms above. Encoder
input conversion uses the encoder delay matrix and additional FIR coefficients
from `silk/resampler.c` and `silk/resampler_rom.c`. Optional original-source
oracles generate synthetic serialization and resampling fixtures; no native
codec source or library is a normal build or test dependency.

Noise-shaped scalar and delayed-decision quantization follow `silk/NSQ.c`,
`silk/NSQ.h` and `silk/NSQ_del_dec.c` at the same pinned revision, under the SILK
terms retained above. The optional `tools/silk_nsq_fixtures.py` linker wrapper
records calls around unchanged original functions during public native SILK
encoding; it does not replace or edit the reference algorithms. Synthetic input,
canonical coefficient arrays and explicit field-wise state fingerprints are
retained for normal Rust tests without linking a native library.

SILK spectral/activity analysis and parameter quantization additionally follow
`silk/fixed/burg_modified_FIX.c`, `silk/fixed/find_LPC_FIX.c`, `silk/A2NLSF.c`,
`silk/process_NLSFs.c`, `silk/NLSF_encode.c`, `silk/NLSF_VQ.c`,
`silk/NLSF_del_dec_quant.c`, `silk/NLSF_VQ_weights_laroia.c`, `silk/gain_quant.c`,
`silk/lin2log.c`, `silk/VAD.c`, `silk/ana_filt_bank_1.c` and `silk/sigm_Q15.c`
at the same revision, under the retained SILK terms. The optional
`tools/silk_analysis_fixtures.py` records unchanged public encoder calls around
these original functions using linker wrappers. Its synthetic workload and
small explicit root probes are validation data, with no C codec build/test
or runtime dependency in the native Rust library.


SILK pitch and LTP encoder analysis additionally follow
`silk/fixed/find_pitch_lags_FIX.c`, `silk/fixed/pitch_analysis_core_FIX.c`,
`silk/fixed/apply_sine_window_FIX.c`, `silk/fixed/schur_FIX.c`,
`silk/fixed/k2a_FIX.c`, `silk/resampler_down2.c`, `silk/resampler_down2_3.c`,
`silk/fixed/find_LTP_FIX.c`, `silk/fixed/corrMatrix_FIX.c`,
`silk/quant_LTP_gains.c`, `silk/VQ_WMat_EC.c`,
`silk/fixed/LTP_analysis_filter_FIX.c`, `silk/log2lin.c` and native complexity
profiles in `silk/control_codec.c`, at the same pinned revision and under the
retained SILK terms. Generated search ranges, code lengths and effective gains
extend the existing pitch/LTP tables. The optional
`tools/silk_pitch_analysis_fixtures.py` wraps unchanged public native encoder
calls and records synthetic pitch/LTP/residual traces for Rust-only tests.


SILK noise shaping and gain processing additionally follow
`silk/fixed/noise_shape_analysis_FIX.c`,
`silk/fixed/warped_autocorrelation_FIX.c`, `silk/fixed/schur64_FIX.c`,
`silk/fixed/k2a_Q16_FIX.c`, `silk/LPC_fit.c`, `silk/bwexpander_32.c`,
`silk/fixed/find_pred_coefs_FIX.c`, `silk/fixed/residual_energy_FIX.c` and
`silk/fixed/process_gains_FIX.c`, at the same pinned revision under the retained
SILK terms. The optional `tools/silk_shaping_fixtures.py` records linked stages
around unchanged public CBR/VBR native encoding, using the synthetic workload
from the existing NSQ oracle and read-only pitch/LTP wrappers. Its traces keep
normal Rust tests independent of a C compiler or native codec library.


The internal SILK channel frame encoder additionally follows
`silk/fixed/encode_frame_FIX.c`, `silk/fixed/LTP_scale_ctrl_FIX.c`,
`silk/gain_quant.c::silk_gains_ID` and native CBR/channel budgeting contracts in
`silk/enc_API.c`, at the same pinned revision under the retained SILK terms.
`tools/silk_encoder_channel_fixtures.py` wraps unchanged full public encoding
calls, transition filtering and frame serialization. It snapshots actual selected
retry states and generates synthetic frame/packet/PCM fingerprints; no original
codec algorithm is replaced and no native source or library enters Cargo.


The fixed-bandwidth mono packet path additionally follows native DC rejection
in `src/opus_encoder.c`, mono sample carry and rate/bit-reservoir policy in
`silk/enc_API.c`, empirical tables/lookup in `silk/control_SNR.c`, and RFC6716
padding as implemented in `src/repacketizer.c`. Existing Opus/SILK notices and
terms above apply. `tools/silk_encoder_rate_tables.py` regenerates the SNR tables;
`tools/silk_mono_validation.py` extracts the unchanged native DC function and
calls the original public decoder for optional validation. Synthetic DC fixtures
and Rust-generated packet traces contain no native codec implementation or
third-party recording; all ordinary Cargo builds/tests remain independent.


The stereo encoder additionally follows `silk/stereo_LR_to_MS.c`,
`silk/stereo_find_predictor.c`, `silk/stereo_quant_pred.c`,
`silk/stereo_encode_pred.c` and stereo scheduling/side reset in `silk/enc_API.c`
at the same pinned revision, under the SILK terms retained above. Immutable
stereo probability/quantizer tables are shared with the existing decoder.
`tools/silk_encoder_stereo_fixtures.py` wraps unchanged public native encoding
calls to capture synthetic PCM, analysis state and entropy bytes. Optional
`silk_stereo_vectors` traces are generated by Rust and checked with the original
public decoder; neither native codec source nor libraries enter Cargo.


SILK encoder DTX follows the inactive-frame and packet flag logic in
`silk/fixed/encode_frame_FIX.c` and `silk/enc_API.c`, with fixed-point digital
silence detection and `decide_dtx_mode` in `src/opus_encoder.c` at the same pinned
revision. Existing SILK/Opus terms above apply. The optional DTX oracle wraps
unchanged public VAD calls and extracts the original outer decision function;
it also retains a minimal original modified-Burg near-silence probe. Captured
synthetic traces and generated packets add no native build/test dependency.

The SILK channel encoder's LBRR branch follows `silk_LBRR_encode_FIX` in
`silk/fixed/encode_frame_FIX.c`, with shared gain reconstruction from
`silk/gain_quant.c` and later packet serialization order from `silk/enc_API.c`,
at the same pinned revision and under the SILK terms above. The optional oracle
extends the existing unchanged public frame wrapper and public decoder calls;
it retains native entropy overflow diagnostics and the actual padded PLC packet,
without substituting codec algorithms or changing reference source.


Fixed-bandwidth FEC frontend policy follows the original `decide_fec` threshold
and hysteresis constants in `src/opus_encoder.c`, LBRR gain increments in
`silk/control_codec.c`, and masks, stereo field ordering and bit-cost averaging
in `silk/enc_API.c` at the same pinned revision. Native automatic bandwidth
selection is outside these fixed-bandwidth APIs. The bounded primary-preserving
packet retry is Opura's admission policy. Existing Opus/SILK terms above apply.
The optional policy oracle extracts unchanged native functions/tables; generated
Rust packet/recovery traces and external library probes are validation only.


SILK bandwidth switching additionally follows `silk/control_audio_bandwidth.c`,
`silk/LP_variable_cutoff.c`, `silk/biquad_alt.c` and the transition coefficients
in `silk/tables_other.c`, at the same pinned revision and SILK terms above.
Channel history migration and resampler priming follow `silk_setup_resamplers`
and prediction resets in `silk_setup_fs`, both in `silk/control_codec.c`.
Optional oracles call unchanged original functions and retain synthetic
PCM/state fingerprints; they add no native build/test dependency.

Connected mono/stereo bandwidth scheduling additionally follows packet activity
permission in `silk/enc_API.c`, per-channel LP placement in
`silk/fixed/encode_frame_FIX.c` and successful-packet readiness acknowledgement in
`src/opus_encoder.c` at the same pinned revision. The optional scheduling oracle
calls the unchanged original control/filter functions and native activity macros;
external reference code remains outside all Cargo builds and ordinary tests.

Automatic fixed-layout SILK bandwidth follows the fixed-point AUDIO default,
`compute_equiv_rate`, automatic bandwidth thresholds/hysteresis, `decide_fec`,
API/hard-rate bounds and outer budget-overflow PLC signaling in
`src/opus_encoder.c` at the same pinned revision. The policy oracle extracts
unchanged original functions/blocks/tables; the overflow oracle wraps original
public encoding and frame calls. Neither adds a native Cargo dependency.

Hybrid encoder rate controls follow `compute_silk_rate_for_hybrid` and the
CBR/VBR SILK cap blocks in `src/opus_encoder.c` at the same pinned revision,
under the existing Opus notices above. The optional oracle extracts unchanged
source functions/blocks into a temporary reference executable; only numerical
fixtures are retained, with no native build or ordinary-test dependency.

Connected Hybrid encoding additionally follows outer `dc_reject`,4ms layer
delay, `HB_gain`, `gain_fade`/`stereo_fade`, shared redundancy-flag accounting and
SILK/CELT rate control in `src/opus_encoder.c`; high-band analysis/control follows
`celt/celt_encoder.c` at the same pinned revision under the existing notices.
Constituent SILK/CELT algorithms retain their attribution above. Optional native
cross-decoding/fade oracles do not link native code into ordinary builds/tests.

Hybrid DTX reuses the SILK inactivity and fixed-point outer silence policy
above, retaining the SILK early return and post-CELT digital-silence ordering in
`src/opus_encoder.c`. The optional forced-Hybrid oracle wraps unchanged public
encoding and counts original SILK/CELT calls; it modifies no codec algorithm.

Hybrid LBRR reuses the SILK redundant NSQ/entropy/mask/stereo/gain policy already
attributed above. Fixed-band Hybrid eligibility follows original Hybrid
`compute_equiv_rate` and `decide_fec` at the pinned revision, admitting protection
only when the original decision retains the requested bandwidth. The native
policy oracle extracts unchanged functions/tables. Whole-stream prefix-free
budget retry is Opura's bounded primary-preserving admission policy.

Hybrid VBR follows the original high-band effective-rate, SILK offset/temporal
bias, transient floor and post-dynalloc/trim minimum-size/shrink blocks in
`celt/celt_encoder.c`, plus VBR SILK caps and high-band target selection in
`src/opus_encoder.c`, at the same pinned revision under the existing notices.
The optional target oracle extracts unchanged native arithmetic blocks. VBR
Hybrid disables native constrained-CELT drift/reservoir feedback; its target
and hard capacity remain separate. Existing constituent attributions apply.

CELT VBR/CVBR target selection, temporal spectrum averaging, initial constrained
capacity and post-control reservoir/drift/count feedback follow the original
scalar fixed-point `compute_vbr` and surrounding blocks in
`celt/celt_encoder.c` at the same pinned revision. The packet-owning API uses
the original CELT signalling deduction for its TOC-inclusive nominal rate.
Native digital silence includes the preceding raw input overlap and resets
active reconstructed energies after quantization. Optional oracles compile
unchanged arithmetic blocks with controlled inputs; no native codec dependency
is introduced. Existing constituent notices and licenses apply.

The borrowing RFC6716 repacketizer follows compatible-TOC grouping, canonical
code0/1/2/3 frame-count/length packing, range selection and exact transport
padding in Opus v1.6.1 `src/repacketizer.c`, copyright (c) 2011 Xiph.Org
Foundation, written by Jean-Marc Valin, under the existing BSD license above.
Native sources remain optional external byte-oracle inputs. Incoming padding
extensions are outside this API. Self-delimited parsing follows Appendix B and
`src/opus.c` at the same pinned revision, copyright (c) 2011 Xiph.Org Foundation,
Skype Limited; 2024 Arm Limited, written by Jean-Marc Valin and Koen Vos.
Surround spectral analysis in `src/celt/analysis/surround.rs` follows
`src/opus_multistream_encoder.c`, copyright (c) 2011 Xiph.Org Foundation,
written by Jean-Marc Valin. Per-stream mask reduction follows the existing
CELT encoder notices (CSIRO, Xiph.Org Foundation and Gregory Maxwell).
The optional oracle retains pristine output hashes alongside explicitly
corrected Q24 return/offset analysis, as documented in
`docs/decisions/0003-surround-q24-precision.md`. Only sanitized numerical
fixtures are tracked; foreign translation units are generated temporarily
from an unchanged external checkout and never enter Cargo or ordinary tests.

Canonical surround construction/rates in `src/multistream/encoder.rs` and mask/LFE
wiring in the common/CELT owners follow the same original multistream encoder,
`src/opus_encoder.c` and `celt/celt_encoder.c` under their existing notices.
`src/encoder/surround.rs` retains the common-owner arithmetic and attribution.
Optional actual call/packet fixtures use external original source with only the
previously disclosed Q24 spectral-patch local-variable correction. The reference
checkout and decoder remain unchanged. No foreign source or codec is a Cargo
build/test dependency.

Channel mapping, stream ordering and range combination follow
`src/opus_multistream.c`, `src/opus_multistream_decoder.c` and the generic layout
checks in `src/opus_multistream_encoder.c`, copyright (c) 2011 Xiph.Org Foundation,
written by Jean-Marc Valin. Decoder signed Q8 gain uses the fixed-point
`src/opus_decoder.c` frame ordering and existing CELT scalar math. The existing
BSD notices above apply; no foreign code is linked or vendored.
Canonical file layout maps in `src/bin/opura/layout.rs` follow the same native
encoder's `vorbis_mappings`, under these notices. Optional mapped file generators
call unchanged native public APIs and use external FFmpeg only to observe channel
order. Tracked Ogg fixtures contain deterministic synthetic audio; neither
foreign source nor executables are included in Cargo or ordinary tests.

Shared scalar encoder frontend decisions and stereo-width analysis follow
`compute_stereo_width`, `compute_equiv_rate`, `decide_fec`, the threshold tables
and normal-admission decision block in Opus v1.6.1 `src/opus_encoder.c`,
copyright (c) 2010-2011 Xiph.Org Foundation, Skype Limited, written by
Jean-Marc Valin and Koen Vos, under the existing BSD notices above. Optional
oracles compile the unchanged external source and extracted decision block;
native code is never included in Cargo builds or ordinary tests.

Distinct CELT API/wire channel processing, spectral downmix, TF channel
selection and reconstructed mono-energy mirroring follow the CC/C paths in
Opus v1.6.1 `celt/celt_encoder.c` under the existing CELT notices. Optional
channel-layout fixtures extend the component oracle's orchestration while
keeping original component functions and public native decoding unchanged.
They do not claim byte parity with the entire native encoder.

CELT variable spectral end/TOC and inactive reconstructed-energy cleanup follow
the end-band and post-frame CC-channel paths in the same pinned
`celt/celt_encoder.c` under the existing notices. Optional coupled band/channel
fixtures retain the original component bodies and public decoder unchanged;
their baseline orchestration is explicit and is not a full native encoder oracle.

The shared encoder PCM input state follows AUDIO `dc_reject` and the outer
10ms `delay_buffer` update block in Opus v1.6.1 `src/opus_encoder.c`, including
4ms CELT alignment and the early SILK-DTX return, under the existing Opus BSD
notices above. The optional PCM oracle includes the unchanged original source
and extracts its history update block in a temporary build; only sanitized
numerical fixtures are tracked. It is never part of Cargo builds or ordinary tests.

Stateful speech wire-layout requests follow the native stereo-to-mono hold in
`src/opus_encoder.c` and channel initialization, shared mid buffering, input
resampler cloning/averaging and incompatible LBRR clearing in `silk/enc_API.c`
at the same pinned Opus revision, under the existing Opus/SILK notices above.
Optional oracles observe unchanged public encoding and VAD input, extract the
unchanged partial stereo initializer, and capture original stereo analysis.
They remain external validation tools, never Cargo build/test dependencies.
