# Opus entropy coding attribution

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

# SILK attribution

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

# CELT scalar math attribution

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

# CELT energy and Laplace attribution

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

# CELT CWRS pulse-vector attribution

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

# CELT PVQ normalization and spreading attribution

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

# CELT FFT attribution

The mixed-radix butterflies, stage scaling and standard plan/twiddle data follow
`celt/kiss_fft.c`, `celt/_kiss_fft_guts.h` and `celt/static_modes_fixed.h` at the
pinned revision above. No C implementation is linked into the Rust library.

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

#ifndef SKIP_CONFIG_H
#  ifdef HAVE_CONFIG_H
#    include "config.h"
#  endif
#endif

#include "_kiss_fft_guts.h"
#include "arch.h"
#include "os_support.h"
#include "mathops.h"
#include "stack_alloc.h"

#ifndef M_PI
#define M_PI 3.141592653
#endif

/* The guts header contains all the multiplication and addition macros that are defined for
   complex numbers.  It also declares the kf_ internal functions.
*/

static void kf_bfly2(
                     kiss_fft_cpx * Fout,
                     int m,
                     int N
                    )
{
   kiss_fft_cpx * Fout2;
   int i;
   (void)m;
#ifdef CUSTOM_MODES
   if (m==1)
   {
      celt_assert(m==1);
      for (i=0;i<N;i++)
      {
         kiss_fft_cpx t;
         Fout2 = Fout + 1;
         t = *Fout2;
         C_SUB( *Fout2 ,  *Fout , t );
         C_ADDTO( *Fout ,  t );
         Fout += 2;
      }
   } else
#endif
   {
      celt_coef tw;
      tw = QCONST32(0.7071067812f, COEF_SHIFT-1);
      /* We know that m==4 here because the radix-2 is just after a radix-4 */
      celt_assert(m==4);
      for (i=0;i<N;i++)
      {
         kiss_fft_cpx t;
         Fout2 = Fout + 4;
         t = Fout2[0];
         C_SUB( Fout2[0] ,  Fout[0] , t );
         C_ADDTO( Fout[0] ,  t );

         t.r = S_MUL(ADD32_ovflw(Fout2[1].r, Fout2[1].i), tw);
         t.i = S_MUL(SUB32_ovflw(Fout2[1].i, Fout2[1].r), tw);
         C_SUB( Fout2[1] ,  Fout[1] , t );
         C_ADDTO( Fout[1] ,  t );

         t.r = Fout2[2].i;
         t.i = NEG32_ovflw(Fout2[2].r);
         C_SUB( Fout2[2] ,  Fout[2] , t );
         C_ADDTO( Fout[2] ,  t );

         t.r = S_MUL(SUB32_ovflw(Fout2[3].i, Fout2[3].r), tw);
         t.i = S_MUL(NEG32_ovflw(ADD32_ovflw(Fout2[3].i, Fout2[3].r)), tw);
         C_SUB( Fout2[3] ,  Fout[3] , t );
         C_ADDTO( Fout[3] ,  t );
         Fout += 8;
      }
   }
}

static void kf_bfly4(
                     kiss_fft_cpx * Fout,
                     const size_t fstride,
                     const kiss_fft_state *st,
                     int m,
                     int N,
                     int mm
                    )
{
   int i;

   if (m==1)
   {
      /* Degenerate case where all the twiddles are 1. */
      for (i=0;i<N;i++)
      {
         kiss_fft_cpx scratch0, scratch1;

         C_SUB( scratch0 , *Fout, Fout[2] );
         C_ADDTO(*Fout, Fout[2]);
         C_ADD( scratch1 , Fout[1] , Fout[3] );
         C_SUB( Fout[2], *Fout, scratch1 );
         C_ADDTO( *Fout , scratch1 );
         C_SUB( scratch1 , Fout[1] , Fout[3] );

         Fout[1].r = ADD32_ovflw(scratch0.r, scratch1.i);
         Fout[1].i = SUB32_ovflw(scratch0.i, scratch1.r);
         Fout[3].r = SUB32_ovflw(scratch0.r, scratch1.i);
         Fout[3].i = ADD32_ovflw(scratch0.i, scratch1.r);
         Fout+=4;
      }
   } else {
      int j;
      kiss_fft_cpx scratch[6];
      const kiss_twiddle_cpx *tw1,*tw2,*tw3;
      const int m2=2*m;
      const int m3=3*m;
      kiss_fft_cpx * Fout_beg = Fout;
      for (i=0;i<N;i++)
      {
         Fout = Fout_beg + i*mm;
         tw3 = tw2 = tw1 = st->twiddles;
         /* m is guaranteed to be a multiple of 4. */
         for (j=0;j<m;j++)
         {
            C_MUL(scratch[0],Fout[m] , *tw1 );
            C_MUL(scratch[1],Fout[m2] , *tw2 );
            C_MUL(scratch[2],Fout[m3] , *tw3 );

            C_SUB( scratch[5] , *Fout, scratch[1] );
            C_ADDTO(*Fout, scratch[1]);
            C_ADD( scratch[3] , scratch[0] , scratch[2] );
            C_SUB( scratch[4] , scratch[0] , scratch[2] );
            C_SUB( Fout[m2], *Fout, scratch[3] );
            tw1 += fstride;
            tw2 += fstride*2;
            tw3 += fstride*3;
            C_ADDTO( *Fout , scratch[3] );

            Fout[m].r = ADD32_ovflw(scratch[5].r, scratch[4].i);
            Fout[m].i = SUB32_ovflw(scratch[5].i, scratch[4].r);
            Fout[m3].r = SUB32_ovflw(scratch[5].r, scratch[4].i);
            Fout[m3].i = ADD32_ovflw(scratch[5].i, scratch[4].r);
            ++Fout;
         }
      }
   }
}


#ifndef RADIX_TWO_ONLY

static void kf_bfly3(
                     kiss_fft_cpx * Fout,
                     const size_t fstride,
                     const kiss_fft_state *st,
                     int m,
                     int N,
                     int mm
                    )
{
   int i;
   size_t k;
   const size_t m2 = 2*m;
   const kiss_twiddle_cpx *tw1,*tw2;
   kiss_fft_cpx scratch[5];
   kiss_twiddle_cpx epi3;

   kiss_fft_cpx * Fout_beg = Fout;
#ifdef FIXED_POINT
   /*epi3.r = -16384;*/ /* Unused */
   epi3.i = -QCONST32(0.86602540f, COEF_SHIFT-1);
#else
   epi3 = st->twiddles[fstride*m];
#endif
   for (i=0;i<N;i++)
   {
      Fout = Fout_beg + i*mm;
      tw1=tw2=st->twiddles;
      /* For non-custom modes, m is guaranteed to be a multiple of 4. */
      k=m;
      do {

         C_MUL(scratch[1],Fout[m] , *tw1);
         C_MUL(scratch[2],Fout[m2] , *tw2);

         C_ADD(scratch[3],scratch[1],scratch[2]);
         C_SUB(scratch[0],scratch[1],scratch[2]);
         tw1 += fstride;
         tw2 += fstride*2;

         Fout[m].r = SUB32_ovflw(Fout->r, HALF_OF(scratch[3].r));
         Fout[m].i = SUB32_ovflw(Fout->i, HALF_OF(scratch[3].i));

         C_MULBYSCALAR( scratch[0] , epi3.i );

         C_ADDTO(*Fout,scratch[3]);

         Fout[m2].r = ADD32_ovflw(Fout[m].r, scratch[0].i);
         Fout[m2].i = SUB32_ovflw(Fout[m].i, scratch[0].r);

         Fout[m].r = SUB32_ovflw(Fout[m].r, scratch[0].i);
         Fout[m].i = ADD32_ovflw(Fout[m].i, scratch[0].r);

         ++Fout;
      } while(--k);
   }
}


#ifndef OVERRIDE_kf_bfly5
static void kf_bfly5(
                     kiss_fft_cpx * Fout,
                     const size_t fstride,
                     const kiss_fft_state *st,
                     int m,
                     int N,
                     int mm
                    )
{
   kiss_fft_cpx *Fout0,*Fout1,*Fout2,*Fout3,*Fout4;
   int i, u;
   kiss_fft_cpx scratch[13];
   const kiss_twiddle_cpx *tw;
   kiss_twiddle_cpx ya,yb;
   kiss_fft_cpx * Fout_beg = Fout;

#ifdef FIXED_POINT
   ya.r = QCONST32(0.30901699f, COEF_SHIFT-1);
   ya.i = -QCONST32(0.95105652f, COEF_SHIFT-1);
   yb.r = -QCONST32(0.80901699f, COEF_SHIFT-1);
   yb.i = -QCONST32(0.58778525f, COEF_SHIFT-1);
#else
   ya = st->twiddles[fstride*m];
   yb = st->twiddles[fstride*2*m];
#endif
   tw=st->twiddles;

   for (i=0;i<N;i++)
   {
      Fout = Fout_beg + i*mm;
      Fout0=Fout;
      Fout1=Fout0+m;
      Fout2=Fout0+2*m;
      Fout3=Fout0+3*m;
      Fout4=Fout0+4*m;

      /* For non-custom modes, m is guaranteed to be a multiple of 4. */
      for ( u=0; u<m; ++u ) {
         scratch[0] = *Fout0;

         C_MUL(scratch[1] ,*Fout1, tw[u*fstride]);
         C_MUL(scratch[2] ,*Fout2, tw[2*u*fstride]);
         C_MUL(scratch[3] ,*Fout3, tw[3*u*fstride]);
         C_MUL(scratch[4] ,*Fout4, tw[4*u*fstride]);

         C_ADD( scratch[7],scratch[1],scratch[4]);
         C_SUB( scratch[10],scratch[1],scratch[4]);
         C_ADD( scratch[8],scratch[2],scratch[3]);
         C_SUB( scratch[9],scratch[2],scratch[3]);

         Fout0->r = ADD32_ovflw(Fout0->r, ADD32_ovflw(scratch[7].r, scratch[8].r));
         Fout0->i = ADD32_ovflw(Fout0->i, ADD32_ovflw(scratch[7].i, scratch[8].i));

         scratch[5].r = ADD32_ovflw(scratch[0].r, ADD32_ovflw(S_MUL(scratch[7].r,ya.r), S_MUL(scratch[8].r,yb.r)));
         scratch[5].i = ADD32_ovflw(scratch[0].i, ADD32_ovflw(S_MUL(scratch[7].i,ya.r), S_MUL(scratch[8].i,yb.r)));

         scratch[6].r =  ADD32_ovflw(S_MUL(scratch[10].i,ya.i), S_MUL(scratch[9].i,yb.i));
         scratch[6].i = NEG32_ovflw(ADD32_ovflw(S_MUL(scratch[10].r,ya.i), S_MUL(scratch[9].r,yb.i)));

         C_SUB(*Fout1,scratch[5],scratch[6]);
         C_ADD(*Fout4,scratch[5],scratch[6]);

         scratch[11].r = ADD32_ovflw(scratch[0].r, ADD32_ovflw(S_MUL(scratch[7].r,yb.r), S_MUL(scratch[8].r,ya.r)));
         scratch[11].i = ADD32_ovflw(scratch[0].i, ADD32_ovflw(S_MUL(scratch[7].i,yb.r), S_MUL(scratch[8].i,ya.r)));
         scratch[12].r = SUB32_ovflw(S_MUL(scratch[9].i,ya.i), S_MUL(scratch[10].i,yb.i));
         scratch[12].i = SUB32_ovflw(S_MUL(scratch[10].r,yb.i), S_MUL(scratch[9].r,ya.i));

         C_ADD(*Fout2,scratch[11],scratch[12]);
         C_SUB(*Fout3,scratch[11],scratch[12]);

         ++Fout0;++Fout1;++Fout2;++Fout3;++Fout4;
      }
   }
}
#endif /* OVERRIDE_kf_bfly5 */


#endif


#ifdef CUSTOM_MODES

static
void compute_bitrev_table(
         int Fout,
         opus_int16 *f,
         const size_t fstride,
         int in_stride,
         opus_int16 * factors,
         const kiss_fft_state *st
            )
{
   const int p=*factors++; /* the radix  */
   const int m=*factors++; /* stage's fft length/p */

    /*printf ("fft %d %d %d %d %d %d\n", p*m, m, p, s2, fstride*in_stride, N);*/
   if (m==1)
   {
      int j;
      for (j=0;j<p;j++)
      {
         *f = Fout+j;
         f += fstride*in_stride;
      }
   } else {
      int j;
      for (j=0;j<p;j++)
      {
         compute_bitrev_table( Fout , f, fstride*p, in_stride, factors,st);
         f += fstride*in_stride;
         Fout += m;
      }
   }
}

/*  facbuf is populated by p1,m1,p2,m2, ...
    where
    p[i] * m[i] = m[i-1]
    m0 = n                  */
static
int kf_factor(int n,opus_int16 * facbuf)
{
    int p=4;
    int i;
    int stages=0;
    int nbak = n;

    /*factor out powers of 4, powers of 2, then any remaining primes */
    do {
        while (n % p) {
            switch (p) {
                case 4: p = 2; break;
                case 2: p = 3; break;
                default: p += 2; break;
            }
            if (p>32000 || (opus_int32)p*(opus_int32)p > n)
                p = n;          /* no more factors, skip to end */
        }
        n /= p;
#ifdef RADIX_TWO_ONLY
        if (p!=2 && p != 4)
#else
        if (p>5)
#endif
        {
           return 0;
        }
        facbuf[2*stages] = p;
        if (p==2 && stages > 1)
        {
           facbuf[2*stages] = 4;
           facbuf[2] = 2;
        }
        stages++;
    } while (n > 1);
    n = nbak;
    /* Reverse the order to get the radix 4 at the end, so we can use the
       fast degenerate case. It turns out that reversing the order also
       improves the noise behaviour. */
    for (i=0;i<stages/2;i++)
    {
       int tmp;
       tmp = facbuf[2*i];
       facbuf[2*i] = facbuf[2*(stages-i-1)];
       facbuf[2*(stages-i-1)] = tmp;
    }
    for (i=0;i<stages;i++)
    {
        n /= facbuf[2*i];
        facbuf[2*i+1] = n;
    }
    return 1;
}

static void compute_twiddles(kiss_twiddle_cpx *twiddles, int nfft)
{
   int i;
#ifdef FIXED_POINT
   for (i=0;i<nfft;++i) {
      opus_val32 phase = -i;
#ifdef ENABLE_QEXT
      twiddles[i].r = (int)MIN32(2147483647, floor(.5+2147483648*cos((2*M_PI/nfft)*phase)));
      twiddles[i].i = (int)MIN32(2147483647, floor(.5+2147483648*sin((2*M_PI/nfft)*phase)));
#else
      kf_cexp2(twiddles+i, DIV32(SHL32(phase,17),nfft));
#endif
   }
#else
   for (i=0;i<nfft;++i) {
      const double pi=3.14159265358979323846264338327;
      double phase = ( -2*pi /nfft ) * i;
      kf_cexp(twiddles+i, phase );
   }
#endif
}

int opus_fft_alloc_arch_c(kiss_fft_state *st) {
   (void)st;
   return 0;
}

/*
 *
 * Allocates all necessary storage space for the fft and ifft.
 * The return value is a contiguous block of memory.  As such,
 * It can be freed with free().
 * */
kiss_fft_state *opus_fft_alloc_twiddles(int nfft,void * mem,size_t * lenmem,
                                        const kiss_fft_state *base, int arch)
{
    kiss_fft_state *st=NULL;
    size_t memneeded = sizeof(struct kiss_fft_state); /* twiddle factors*/

    if ( lenmem==NULL ) {
        st = ( kiss_fft_state*)KISS_FFT_MALLOC( memneeded );
    }else{
        if (mem != NULL && *lenmem >= memneeded)
            st = (kiss_fft_state*)mem;
        *lenmem = memneeded;
    }
    if (st) {
        opus_int16 *bitrev;
        kiss_twiddle_cpx *twiddles;

        st->nfft=nfft;
#ifdef FIXED_POINT
        st->scale_shift = celt_ilog2(st->nfft);
# ifdef ENABLE_QEXT
        if (st->nfft == 1<<st->scale_shift)
           st->scale = QCONST32(1.0f, 30);
        else
           st->scale = (((opus_int64)1073741824<<st->scale_shift)+st->nfft/2)/st->nfft;
# else
        if (st->nfft == 1<<st->scale_shift)
           st->scale = Q15ONE;
        else
           st->scale = (1073741824+st->nfft/2)/st->nfft>>(15-st->scale_shift);
# endif
#else
        st->scale = 1.f/nfft;
#endif
        if (base != NULL)
        {
           st->twiddles = base->twiddles;
           st->shift = 0;
           while (st->shift < 32 && nfft<<st->shift != base->nfft)
              st->shift++;
           if (st->shift>=32)
              goto fail;
        } else {
           st->twiddles = twiddles = (kiss_twiddle_cpx*)KISS_FFT_MALLOC(sizeof(kiss_twiddle_cpx)*nfft);
           compute_twiddles(twiddles, nfft);
           st->shift = -1;
        }
        if (!kf_factor(nfft,st->factors))
        {
           goto fail;
        }

        /* bitrev */
        st->bitrev = bitrev = (opus_int16*)KISS_FFT_MALLOC(sizeof(opus_int16)*nfft);
        if (st->bitrev==NULL)
            goto fail;
        compute_bitrev_table(0, bitrev, 1,1, st->factors,st);

        /* Initialize architecture specific fft parameters */
        if (opus_fft_alloc_arch(st, arch))
            goto fail;
    }
    return st;
fail:
    opus_fft_free(st, arch);
    return NULL;
}

kiss_fft_state *opus_fft_alloc(int nfft,void * mem,size_t * lenmem, int arch)
{
   return opus_fft_alloc_twiddles(nfft, mem, lenmem, NULL, arch);
}

void opus_fft_free_arch_c(kiss_fft_state *st) {
   (void)st;
}

void opus_fft_free(const kiss_fft_state *cfg, int arch)
{
   if (cfg)
   {
      opus_fft_free_arch((kiss_fft_state *)cfg, arch);
      opus_free((opus_int16*)cfg->bitrev);
      if (cfg->shift < 0)
         opus_free((kiss_twiddle_cpx*)cfg->twiddles);
      opus_free((kiss_fft_state*)cfg);
   }
}

#endif /* CUSTOM_MODES */

#ifdef FIXED_POINT
#ifndef OVERRIDE_fft_downshift
static void fft_downshift(kiss_fft_cpx *x, int N, int *total, int step) {
   int shift;
   shift = IMIN(step, *total);
   *total -= shift;
   if (shift == 1) {
      int i;
      for (i=0;i<N;i++) {
         x[i].r = SHR32(x[i].r, 1);
         x[i].i = SHR32(x[i].i, 1);
      }
   } else if (shift>0) {
      int i;
      for (i=0;i<N;i++) {
         x[i].r = PSHR32(x[i].r, shift);
         x[i].i = PSHR32(x[i].i, shift);
      }
   }
}
#endif /* OVERRIDE_fft_downshift */
#else
#define fft_downshift(x, N, total, step)
#endif

void opus_fft_impl(const kiss_fft_state *st,kiss_fft_cpx *fout ARG_FIXED(int downshift))
{
    int m2, m;
    int p;
    int L;
    int fstride[MAXFACTORS];
    int i;
    int shift;

    /* st->shift can be -1 */
    shift = st->shift>0 ? st->shift : 0;

    fstride[0] = 1;
    L=0;
    do {
       p = st->factors[2*L];
       m = st->factors[2*L+1];
       fstride[L+1] = fstride[L]*p;
       L++;
    } while(m!=1);
    m = st->factors[2*L-1];
    for (i=L-1;i>=0;i--)
    {
       if (i!=0)
          m2 = st->factors[2*i-1];
       else
          m2 = 1;
       switch (st->factors[2*i])
       {
       case 2:
          fft_downshift(fout, st->nfft, &downshift, 1);
          kf_bfly2(fout, m, fstride[i]);
          break;
       case 4:
          fft_downshift(fout, st->nfft, &downshift, 2);
          kf_bfly4(fout,fstride[i]<<shift,st,m, fstride[i], m2);
          break;
 #ifndef RADIX_TWO_ONLY
       case 3:
          fft_downshift(fout, st->nfft, &downshift, 2);
          kf_bfly3(fout,fstride[i]<<shift,st,m, fstride[i], m2);
          break;
       case 5:
          fft_downshift(fout, st->nfft, &downshift, 3);
          kf_bfly5(fout,fstride[i]<<shift,st,m, fstride[i], m2);
          break;
 #endif
       }
       m = m2;
    }
    fft_downshift(fout, st->nfft, &downshift, downshift);
}

void opus_fft_c(const kiss_fft_state *st,const kiss_fft_cpx *fin,kiss_fft_cpx *fout)
{
   int i;
   celt_coef scale;
#ifdef FIXED_POINT
   /* Allows us to scale with MULT16_32_Q16(), which is faster than
      MULT16_32_Q15() on ARM. */
   int scale_shift = st->scale_shift-1;
#endif
   scale = st->scale;

   celt_assert2 (fin != fout, "In-place FFT not supported");
   /* Bit-reverse the input */
   for (i=0;i<st->nfft;i++)
   {
      kiss_fft_cpx x = fin[i];
      fout[st->bitrev[i]].r = S_MUL2(x.r, scale);
      fout[st->bitrev[i]].i = S_MUL2(x.i, scale);
   }
   opus_fft_impl(st, fout ARG_FIXED(scale_shift));
}


void opus_ifft_c(const kiss_fft_state *st,const kiss_fft_cpx *fin,kiss_fft_cpx *fout)
{
   int i;
   celt_assert2 (fin != fout, "In-place FFT not supported");
   /* Bit-reverse the input */
   for (i=0;i<st->nfft;i++)
      fout[st->bitrev[i]] = fin[i];
   for (i=0;i<st->nfft;i++)
      fout[i].i = -fout[i].i;
   opus_fft_impl(st, fout ARG_FIXED(0));
   for (i=0;i<st->nfft;i++)
      fout[i].i = -fout[i].i;
}
```

# CELT forward/inverse MDCT attribution

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

#ifndef SKIP_CONFIG_H
```

# CELT spectral scaling and de-emphasis attribution

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

# CELT comb postfilter attribution

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

# CELT synthesis backend ordering

Channel projection, transient block scheduling and synthesis clipping follow
`celt/celt_decoder.c` at the pinned revision above, under its retained original
CSIRO/Xiph.Org/Gregory Maxwell notice. The composed scalar, CWRS/PVQ, transform,
band scaling, postfilter and de-emphasis stages retain their notices above.

# CELT control and TF attribution

Prefix/silence accounting, postfilter/transient/intra guards and TF/select/spread
decoding follow `celt/celt_decoder.c`; TF mappings follow `celt/celt.c` and
probability models follow `celt/celt.h` at the pinned revision. Their original
CSIRO/Xiph.Org/Gregory Maxwell notices and redistribution terms are retained above.

# CELT bit allocation attribution

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


# CELT band arithmetic attribution

Coded theta/log2tan, Haar, Hadamard ordering and stereo merge follow
`celt/bands.c`; folded/noise normalization follows `celt/vq.c` at the pinned
revision above. Their original CSIRO/Xiph.Org/Gregory Maxwell notices and
redistribution terms are retained above. The optional oracle extracts these
functions unchanged into temporary developer probes; no C is linked into Cargo.


# CELT recursive partition attribution

Theta resolution/entropy, recursive partition/gain/bit balancing and noise/folding
follow `celt/bands.c`; pulse mapping follows `celt/rate.h` and standard cache
indices/bits follow `celt/static_modes_fixed.h` at the pinned revision above.
Their original CSIRO/Xiph.Org/Gregory Maxwell notices and terms are retained above.


# CELT mono/stereo band orchestration attribution

TF/Haar/Hadamard scheduling, folding output, one-coordinate signs and stereo
angle/intensity/inversion/N2/merge scheduling follow `celt/bands.c` at the pinned
revision above. Original CSIRO/Xiph.Org/Gregory Maxwell notices and redistribution
terms are retained above; existing scalar/PVQ/transform notices also apply.


# CELT all-band spectral assembly attribution

Cross-band balance, folding selection/masks, special Hybrid folding and
stereo-mode transitions follow `quant_all_bands` / `special_hybrid_folding` in
`celt/bands.c` at the pinned revision above, under the retained original
CSIRO/Xiph.Org/Gregory Maxwell notices and redistribution terms.


# CELT transient anti-collapse attribution

Bit-depth/energy attenuation, mono history merging, short-block noise and
renormalization scheduling follow `anti_collapse` in `celt/bands.c` at the pinned
revision above, under the retained CSIRO/Xiph.Org/Gregory Maxwell notices and
redistribution terms. Existing scalar math and vector normalization notices apply.


# Normal CELT decoder integration attribution

Normal-frame stage ordering, channel/energy/history/RNG and silence policy follow
`celt/celt_decoder.c`; bandwidth/final-range packet behavior follows
`src/opus_decoder.c` at the pinned revision above. Existing original retained
CSIRO/Xiph.Org/Gregory Maxwell decoder notices and component terms apply.
The optional complete-packet oracle appends read-only snapshot functions to
original external decoder files, preserving all original arithmetic statements
and notices. No reference C implementation is linked into Cargo.


# CELT autocorrelation, LPC and filters attribution

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


# CELT PLC pitch analysis attribution

Classic downsampling/whitening, FIR5, coarse/fine correlations, normalized pitch
ranking and pseudo interpolation follow `celt/pitch.c`; fixed PLC search geometry
follows `celt/celt_decoder.c` at the pinned revision above. Their original
CSIRO/Xiph.Org/Jean-Marc Valin notices and terms are retained above; existing
LPC and scalar math attribution also apply.


# CELT concealment and recovery attribution

Periodic/noise PLC selection, LPC stabilization, excitation/synthesis energy
checks, noise/background decay, prefilter/TDAC, loss counters and recovery follow
`celt/celt_decoder.c`; public loss/DTX scheduling and initial silence follow
`src/opus_decoder.c` at the pinned revision above. Their existing original
CSIRO/Xiph.Org/Gregory Maxwell notices and terms are retained above, along with
LPC, pitch, scalar math, comb and MDCT component notices. Optional guard oracles
extract unchanged original safety blocks into temporary probes only.


# Hybrid packet integration attribution

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


# Unified Opus decoder attribution

All-mode packet/loss/FEC orchestration, primary entropy-storage shrinking,
redundant CELT-tail ordering, mode resets, squared-window PCM fades and final
range XOR follow `src/opus_decoder.c`; storage behavior follows `celt/entdec.c`
and window/fixed-point arithmetic follows the CELT sources identified above.
The original Xiph.Org Foundation/Skype Limited/Arm Limited packet integration
notice and all SILK/CELT component notices and terms are retained above.


# Optional official conformance validation

The RFC8251 vector archive is an external validation input, verified against
its official file hashes and a pinned archive SHA256. It is not redistributed
in the repository or fetched by Cargo. The optional runner compiles the
unchanged external `src/opus_compare.c` at the pinned Opus revision; its original
Xiph.Org Foundation/Mozilla Corporation notice and BSD terms remain in that
external source. No comparator or reference codec is linked into Opura.


# CELT input pre-emphasis attribution

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


# CELT channel/subframe analysis attribution

Long/short MDCT orchestration, stereo-to-mono rounding and low-rate spectral
scaling/cutoff follow `celt/celt_encoder.c::compute_mdcts` at the pinned revision.
The CELT encoder notices and terms retained in the preceding input section apply;
individual transforms retain the forward/inverse MDCT and KISS FFT notices above.


# CELT encoder band analysis attribution

Band amplitudes and normalization follow `celt/bands.c::compute_band_energies`
and `normalise_bands`; mean-adjusted log energies follow
`celt/quant_bands.c::amp2Log2`. Refined square root and normalized reciprocal
follow `celt/mathops.c::celt_sqrt32`, `celt_rcp_norm16` and `celt_rcp_norm32`.
All use the pinned revision above, with their CELT band/scalar notices and terms
already retained here. Canonical band edges, log widths and means keep their
existing mode/table attribution. Active-domain overflow validation is added in Rust.


# CELT encoder energy quantization attribution

Coarse temporal/inter-band prediction, decay/scarcity/LFE policy, inter/intra
RDO and fine/final refinement follow `celt/quant_bands.c` at the pinned revision.
The CELT energy/probability/quant_bands notices and terms already retained above
apply. Rust adds atomic stage failures, checked physical-target arithmetic and
deterministic zero residuals outside each newly selected coarse layout; original
unused per-frame scratch is not treated as initialized state by optional probes.


# CELT CWRS encoding attribution

Vector ranking and uniform pulse encoding follow `celt/cwrs.c::icwrs` and
`encode_pulses` at the pinned revision above. Existing CWRS notices and terms
apply. Optional encoder fixtures use the unchanged original SMALL_FOOTPRINT
recurrence so every supported representable geometry is covered, including
geometries outside the optimized standard-mode table. Zero-pulse encoding is
a no-bit extension paired with Opura's existing zero-pulse decoding contract.


# CELT PVQ pulse search and encoding attribution

Forward spreading, adaptive-headroom pyramid/greedy search, strict score ties,
silence fallback, sign restoration and resynthesis follow `celt/vq.c::alg_quant`,
`op_pvq_search_c`, `exp_rotation` and `normalise_residual` at the pinned revision.
Existing normalized-band/vq notices and terms apply. Rust validates a bounded
normalized Q24 input domain, retains immutable caller input, and stages returned
reconstruction before the single atomic CWRS write. QEXT refinement is excluded.


# CELT recursive mono encoding attribution

Measured energy-ratio angles and recursive mono encoding follow
`celt/vq.c::stereo_itheta` (mono path) and `celt/bands.c::compute_theta` /
`quant_partition` at the pinned revision above. Normalized arctangent functions
follow `celt/mathops.h::celt_atan_norm` and `celt_atan2p_norm`. Existing band/vq/
scalar notices and terms apply. Rust shares unchanged zero-pulse reconstruction
with decoding, validates bounded immutable input and commits complete operations
atomically. Stereo/TF band encoding is not supplied by this mono primitive.


# CELT stereo and time-frequency band encoding attribution

Stereo energy-ratio measurement, amplitude weighting, mid/side transforms,
angle candidate/model/inversion policy, signs and TF band orchestration follow
`celt/vq.c::stereo_itheta` and `celt/bands.c::intensity_stereo`, `stereo_split`,
`compute_theta`, `quant_band_n1`, `quant_band` and `quant_band_stereo` at the pinned
revision. Existing CELT band/vq/scalar notices and terms apply. Rust shares band
validation and TF reconstruction between entropy directions, validates bounded
immutable normalized inputs and commits complete operations atomically.


# CELT complete-spectrum encoding and theta RDO attribution

Full-band budget/folding/channel orchestration and two-candidate stereo angle
selection follow `celt/bands.c::quant_all_bands` and `compute_channel_weights` at
the pinned revision. Existing CELT bands/vq/scalar notices and terms apply.
Rust shares a statically dispatched spectral driver between entropy directions,
validates bounded immutable input/amplitude domains and stages full writer/output.
Optional original evidence enables RESYNTH and uses read-only entropy-symbol
observers around prefix decoding; original codec function bodies stay unchanged.


# CELT TF and spreading control encoding attribution

Differential/reserved-select TF writing follows `celt/celt_encoder.c::tf_encode`
at the pinned revision. Spreading-field coding follows its spread ICDF budget
check; existing CELT encoder/control/table notices and terms apply. Rust accepts
immutable binary decisions and returns effective resolutions, validates frame/
writer domains and stages the whole operation. TF/spread analysis is separate.


# CELT allocation encoding attribution

Dynamic boost and trim signalling follow `celt/celt_encoder.c`; retained-band
hysteresis and stereo decisions follow `celt/rate.c::interp_bits2pulses` /
`clt_compute_allocation` at the pinned revision. Existing CELT encoder/rate/table
notices and terms apply. Rust shares mandatory numerical allocation math with
decoding, validates immutable requested policies and stages whole writer commits.
Optional original probes reuse a deterministic project-owned entropy prefix helper;
no reference codec code is linked into ordinary builds/tests.


# Stateful CELT PCM packet encoder attribution

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


# CELT tone/transient analysis attribution

`src/celt/analysis/transient.rs` and its constants follow `tone_detect`,
`tone_lpc`, `transient_analysis`, `acos_approx` and their inverse table in
`celt/celt_encoder.c` at the pinned revision. Existing CELT encoder notices and
terms apply. Private wide-Q29 division follows `celt/mathops.c::frac_div32_q29`;
its public fractional-domain contract remains unchanged. Rust bounds immutable
physical input and uses fixed safe storage. Optional packet selection and the
anti-collapse streak follow original encoder control flow. The external metric
observer cross-checks all ordinary outputs against unchanged original analysis.


# CELT time/frequency analysis attribution

`src/celt/analysis/tf.rs` follows `celt/celt_encoder.c::l1_metric` and
`tf_analysis` at the pinned revision, reusing original `haar1` arithmetic and
`tf_select_table` through the existing Rust stages/tables. Existing CELT encoder,
band and control notices and terms apply. Rust adds bounded immutable normalized
input, fixed storage and typed validation. Packet integration follows native
budget/tone gating and switching cost with explicit neutral importance. The
optional read-only metric observer cross-checks ordinary decisions/select
against unchanged original analysis; native code is never a Cargo dependency.


# CELT perceptual dynamic-allocation analysis attribution

`src/celt/analysis/dynalloc.rs` and canonical constants follow
`celt/celt_encoder.c::dynalloc_analysis`, `median_of_3` and `median_of_5` at the
pinned revision. Existing CELT encoder/rate/math/table notices and terms apply.
Importance uses the existing classic exponent path selected by the original
non-QEXT build. Rust bounds immutable logs/controls, retains exact raw cap-hit
requests, and supplies counts clamped to 0..255 with a standard-cap wire proof.
Optional packet integration follows native transient long-window comparison,
energy-history ordering and perceptual TF weighting under explicit CBR controls.
All reference code remains an optional external validation tool.


# CELT pulse spreading analysis attribution

`src/celt/analysis/spreading.rs` follows `celt/bands.c::spreading_decision` at
the pinned revision. Existing CELT band/encoder notices and terms apply. Native
initial histories are observed through unchanged encoder initialization. Rust
bounds immutable normalized inputs/weights and checked histories, committing
all state atomically. Optional packet selection follows the original spreading
budget/short-window/byte gates and fallback history, using native masking weights.
Reference algorithms remain optional external validation and never enter Cargo.


# CELT trim and stereo allocation analysis attribution

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


# CELT encoder pitch analysis attribution

`src/celt/pitch/encoder.rs` follows unchanged `celt/pitch.c::remove_doubling`
and `compute_pitch_gain`; shared private downsampling/whitening/search geometry
additionally follows `celt/celt_encoder.c::run_prefilter` at the pinned revision.
Existing CSIRO/Xiph.Org/Jean-Marc Valin pitch, LPC and scalar-math notices and
terms apply. Rust checks geometry/history/signal and accumulator domains with
immutable inputs and fixed storage. Original scalar/context observers remain
optional external validation; no reference implementation enters Cargo.
