# Opus entropy coding attribution

The Rust range decoder follows the algorithms and integer arithmetic of
RFC 6716 and the Xiph.Org Opus entropy coder (entdec.c and entcode.c).
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
