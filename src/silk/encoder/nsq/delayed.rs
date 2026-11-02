//! Native delayed-decision beam search, warped shaping and delayed state commits.
use super::*;
const DELAY: usize = 40;
#[derive(Clone, Copy)]
struct Decision {
    lpc: [i32; 96],
    random: [i32; DELAY],
    q: [i32; DELAY],
    xq: [i32; DELAY],
    prediction: [i32; DELAY],
    shape: [i32; DELAY],
    ar: [i32; 24],
    low: i32,
    difference: i32,
    seed: i32,
    initial: i32,
    cost: i32,
}
impl Default for Decision {
    fn default() -> Self {
        Self {
            lpc: [0; 96],
            random: [0; DELAY],
            q: [0; DELAY],
            xq: [0; DELAY],
            prediction: [0; DELAY],
            shape: [0; DELAY],
            ar: [0; 24],
            low: 0,
            difference: 0,
            seed: 0,
            initial: 0,
            cost: 0,
        }
    }
}
#[derive(Clone, Copy, Default)]
struct Sample {
    q: i32,
    cost: i32,
    xq: i32,
    low: i32,
    difference: i32,
    shape: i32,
    excitation: i32,
}
fn winner(states: &[Decision]) -> usize {
    let mut best = 0;
    for k in 1..states.len() {
        if states[k].cost < states[best].cost {
            best = k;
        }
    }
    best
}
// These kernels mirror the reference's per-frame, per-subframe buffer ownership.
#[allow(clippy::too_many_arguments)]
pub(super) fn process(
    s: &mut NoiseShapingQuantizer,
    khz: usize,
    subframes: usize,
    input: &[i16],
    p: &NsqParameters,
    count: usize,
    warp: i16,
) -> NsqFrame {
    let memory = 20 * khz;
    let sub_length = 5 * khz;
    let order = if khz == 16 { 16 } else { 10 };
    let mut frame = NsqFrame {
        pulses: [0; 320],
        pcm: [0; 320],
        length: input.len(),
        seed: p.seed,
    };
    let mut decisions = [Decision::default(); 4];
    let dd = &mut decisions[..count];
    for (k, d) in dd.iter_mut().enumerate() {
        d.seed = (k as i32 + i32::from(p.seed)) & 3;
        d.initial = d.seed;
        d.low = s.low_frequency;
        d.difference = s.difference;
        d.shape[0] = s.shaping_ltp[memory - 1];
        d.lpc[..16].copy_from_slice(&s.lpc[..16]);
        d.ar = s.ar;
    }
    let mut lag = s.previous_lag;
    let mut delay = DELAY.min(sub_length);
    if p.signal == SignalType::Voiced {
        for &pitch in &p.pitch[..subframes] {
            delay = delay.min(usize::from(pitch) - 3);
        }
    } else if lag > 0 {
        delay = delay.min(lag - 3);
    }
    let interpolation = usize::from(p.interpolation_q2 != 4);
    let mut ring = 0;
    let mut relative_subframe = 0;
    let mut ltp = [0i32; 640];
    let mut white = [0i16; 640];
    let mut scaled = [0i32; 80];
    let mut gain_delay = [0i32; DELAY];
    s.shaping_index = memory;
    s.ltp_index = memory;
    for k in 0..subframes {
        let predictor = &p.predictor_q12[(k >> 1) | (1 - interpolation)];
        s.rewhite = false;
        if p.signal == SignalType::Voiced {
            lag = usize::from(p.pitch[k]);
            if k & (3 - 2 * interpolation) == 0 {
                if k == 2 {
                    let best = winner(dd);
                    for (j, d) in dd.iter_mut().enumerate() {
                        if j != best {
                            d.cost = d.cost.wrapping_add(i32::MAX >> 4);
                        }
                    }
                    flush(
                        s,
                        &dd[best],
                        &mut frame,
                        ring,
                        delay,
                        k * sub_length,
                        p.gains_q16[1],
                        14,
                    );
                    relative_subframe = 0;
                }
                let start = memory - lag - order - 2;
                analysis_filter(
                    &mut white[start..memory],
                    &s.xq[start + k * sub_length..memory + k * sub_length],
                    &predictor[..order],
                );
                s.ltp_index = memory;
                s.rewhite = true;
            }
        }
        scale(
            s,
            dd,
            k,
            memory,
            delay,
            p,
            &input[k * sub_length..(k + 1) * sub_length],
            &mut scaled[..sub_length],
            &white,
            &mut ltp,
        );
        samples(
            s,
            dd,
            k,
            relative_subframe,
            order,
            lag,
            delay,
            warp,
            p,
            &scaled[..sub_length],
            predictor,
            &mut ltp,
            &mut gain_delay,
            &mut ring,
            &mut frame,
        );
        relative_subframe += 1;
    }
    let best = winner(dd);
    let d = &dd[best];
    frame.seed = d.initial as u8;
    let length = frame.length;
    flush(
        s,
        d,
        &mut frame,
        ring,
        delay,
        length,
        p.gains_q16[subframes - 1] >> 6,
        8,
    );
    s.lpc[..16].copy_from_slice(&d.lpc[sub_length..sub_length + 16]);
    s.ar = d.ar;
    s.low_frequency = d.low;
    s.difference = d.difference;
    s.previous_lag = usize::from(p.pitch[subframes - 1]);
    frame.pcm[..frame.length].copy_from_slice(&s.xq[memory..memory + frame.length]);
    s.xq.copy_within(frame.length..frame.length + memory, 0);
    s.shaping_ltp
        .copy_within(frame.length..frame.length + memory, 0);
    frame
}
#[allow(clippy::too_many_arguments)]
fn flush(
    s: &mut NoiseShapingQuantizer,
    d: &Decision,
    frame: &mut NsqFrame,
    ring: usize,
    delay: usize,
    end: usize,
    gain: i32,
    bits: u32,
) {
    let memory = s.rate.map_or(0, |rate| rate.khz() as usize * 20);
    let mut last = (ring + delay) % DELAY;
    for i in 0..delay {
        last = (last + DELAY - 1) % DELAY;
        frame.pulses[end - delay + i] = round_shift(d.q[last], 10) as i8;
        s.xq[memory + end - delay + i] = saturate16(round_shift(mul_ww(d.xq[last], gain), bits));
        s.shaping_ltp[s.shaping_index - delay + i] = d.shape[last];
    }
}
#[allow(clippy::too_many_arguments)]
fn scale(
    s: &mut NoiseShapingQuantizer,
    dd: &mut [Decision],
    k: usize,
    memory: usize,
    delay: usize,
    p: &NsqParameters,
    input: &[i16],
    scaled: &mut [i32],
    white: &[i16],
    ltp: &mut [i32],
) {
    let lag = usize::from(p.pitch[k]);
    let mut inverse = inverse_gain(p.gains_q16[k], 47);
    let inverse26 = round_shift(inverse, 5);
    for (out, &sample) in scaled.iter_mut().zip(input) {
        *out = mul_ww(i32::from(sample), inverse26);
    }
    if s.rewhite {
        if k == 0 {
            inverse = mul_wb(inverse, i32::from(p.ltp_scale_q14)).wrapping_shl(2);
        }
        for i in s.ltp_index - lag - 2..s.ltp_index {
            ltp[i] = mul_wb(inverse, i32::from(white[i]));
        }
    }
    if p.gains_q16[k] != s.previous_gain {
        let adjustment = divide_gain(s.previous_gain, p.gains_q16[k]);
        for v in &mut s.shaping_ltp[s.shaping_index - memory..s.shaping_index] {
            *v = mul_ww(adjustment, *v);
        }
        if p.signal == SignalType::Voiced && !s.rewhite {
            for v in &mut ltp[s.ltp_index - lag - 2..s.ltp_index - delay] {
                *v = mul_ww(adjustment, *v);
            }
        }
        for d in dd {
            d.low = mul_ww(adjustment, d.low);
            d.difference = mul_ww(adjustment, d.difference);
            for v in &mut d.lpc[..16] {
                *v = mul_ww(adjustment, *v);
            }
            for v in &mut d.ar {
                *v = mul_ww(adjustment, *v);
            }
            for v in &mut d.prediction {
                *v = mul_ww(adjustment, *v);
            }
            for v in &mut d.shape {
                *v = mul_ww(adjustment, *v);
            }
        }
        s.previous_gain = p.gains_q16[k];
    }
}
#[allow(clippy::too_many_arguments)]
fn samples(
    s: &mut NoiseShapingQuantizer,
    dd: &mut [Decision],
    k: usize,
    subframe: usize,
    order: usize,
    lag: usize,
    delay: usize,
    warp: i16,
    p: &NsqParameters,
    input: &[i32],
    predictor: &[i16; 16],
    ltp: &mut [i32],
    gains: &mut [i32; DELAY],
    ring: &mut usize,
    frame: &mut NsqFrame,
) {
    let harmonic = i32::from(p.harmonic_q14[k]);
    let packed = (harmonic >> 2) | ((harmonic >> 1) << 16);
    let offset =
        [[100, 240], [32, 100]][usize::from(p.signal == SignalType::Voiced)][usize::from(p.offset)];
    let mut pairs = [[Sample::default(); 2]; 4];
    let memory = s.rate.map_or(0, |rate| rate.khz() as usize * 20);
    for (i, &sample) in input.iter().enumerate() {
        let mut long = 0i32;
        if p.signal == SignalType::Voiced {
            long = 2;
            for j in 0..5 {
                long = long.wrapping_add(mul_wb(
                    ltp[s.ltp_index - lag + 2 - j],
                    i32::from(p.ltp_q14[k][j]),
                ));
            }
            long = long.wrapping_shl(1);
        }
        let long_shape = if lag > 0 {
            let at = s.shaping_index - lag + 1;
            let value = mul_wb(
                s.shaping_ltp[at].saturating_add(s.shaping_ltp[at - 2]),
                packed,
            )
            .wrapping_add(mul_wt(s.shaping_ltp[at - 1], packed));
            long.wrapping_sub(value.wrapping_shl(2))
        } else {
            0
        };
        for (j, d) in dd.iter_mut().enumerate() {
            d.seed = d.seed.wrapping_mul(196314165).wrapping_add(907633515);
            let mut short = (order / 2) as i32;
            for (c, &coefficient) in predictor[..order].iter().enumerate() {
                short = short.wrapping_add(mul_wb(d.lpc[15 + i - c], i32::from(coefficient)));
            }
            short = short.wrapping_shl(4);
            let ar = warped_feedback(
                d,
                &p.shaping_q13[k][..usize::from(p.shaping_order)],
                warp,
                p.tilt_q14[k],
            );
            let low = mul_wb(d.shape[*ring], p.low_frequency_q14[k])
                .wrapping_add(mul_wt(d.low, p.low_frequency_q14[k]))
                .wrapping_shl(2);
            let prediction = round_shift(
                long_shape
                    .wrapping_add(short)
                    .saturating_sub(ar.saturating_add(low)),
                4,
            );
            let mut residual = sample.wrapping_sub(prediction);
            if d.seed < 0 {
                residual = residual.wrapping_neg();
            }
            residual = residual.clamp(-(31 << 10), 30 << 10);
            let [first, second] = candidates(residual, offset, i32::from(p.lambda_q10));
            let choices = if first.1 >> 10 < second.1 >> 10 {
                [first, second]
            } else {
                [second, first]
            };
            for (choice, &(q, cost)) in choices.iter().enumerate() {
                let mut excitation = q.wrapping_shl(4);
                if d.seed < 0 {
                    excitation = excitation.wrapping_neg();
                }
                let excitation = excitation.wrapping_add(long);
                let xq = excitation.wrapping_add(short);
                let difference = xq.wrapping_sub(sample.wrapping_shl(4));
                let low_state = difference.wrapping_sub(ar);
                pairs[j][choice] = Sample {
                    q,
                    cost: d.cost.wrapping_add(cost >> 10),
                    xq,
                    low: low_state,
                    difference,
                    shape: low_state.saturating_sub(low),
                    excitation,
                };
            }
        }
        *ring = (*ring + DELAY - 1) % DELAY;
        let last = (*ring + delay) % DELAY;
        let mut best = 0;
        for j in 1..dd.len() {
            if pairs[j][0].cost < pairs[best][0].cost {
                best = j;
            }
        }
        let winning_random = dd[best].random[last];
        for (j, d) in dd.iter().enumerate() {
            if d.random[last] != winning_random {
                for sample in &mut pairs[j] {
                    sample.cost = sample.cost.wrapping_add(i32::MAX >> 4);
                }
            }
        }
        let mut worst = 0;
        let mut alternative = 0;
        for j in 1..dd.len() {
            if pairs[j][0].cost > pairs[worst][0].cost {
                worst = j;
            }
            if pairs[j][1].cost < pairs[alternative][1].cost {
                alternative = j;
            }
        }
        if pairs[alternative][1].cost < pairs[worst][0].cost {
            let old_lpc = dd[worst].lpc;
            dd[worst] = dd[alternative];
            dd[worst].lpc[..i].copy_from_slice(&old_lpc[..i]);
            pairs[worst][0] = pairs[alternative][1];
        }
        if subframe > 0 || i >= delay {
            let d = &dd[best];
            let position = k * input.len() + i - delay;
            frame.pulses[position] = round_shift(d.q[last], 10) as i8;
            s.xq[memory + position] = saturate16(round_shift(mul_ww(d.xq[last], gains[last]), 8));
            s.shaping_ltp[s.shaping_index - delay] = d.shape[last];
            ltp[s.ltp_index - delay] = d.prediction[last];
        }
        s.shaping_index += 1;
        s.ltp_index += 1;
        for (j, d) in dd.iter_mut().enumerate() {
            let sample = pairs[j][0];
            d.low = sample.low;
            d.difference = sample.difference;
            d.lpc[16 + i] = sample.xq;
            d.xq[*ring] = sample.xq;
            d.q[*ring] = sample.q;
            d.prediction[*ring] = sample.excitation.wrapping_shl(1);
            d.shape[*ring] = sample.shape;
            d.seed = d.seed.wrapping_add(round_shift(sample.q, 10));
            d.random[*ring] = d.seed;
            d.cost = sample.cost;
        }
        gains[*ring] = p.gains_q16[k] >> 6;
    }
    for d in dd {
        d.lpc.copy_within(input.len()..input.len() + 16, 0);
    }
}
fn warped_feedback(d: &mut Decision, coefficients: &[i16], warp: i16, tilt: i16) -> i32 {
    let mut second = d.difference.wrapping_add(mul_wb(d.ar[0], i32::from(warp)));
    let mut first = d.ar[0].wrapping_add(mul_wb(d.ar[1].wrapping_sub(second), i32::from(warp)));
    d.ar[0] = second;
    let mut sum = (coefficients.len() / 2) as i32;
    sum = sum.wrapping_add(mul_wb(second, i32::from(coefficients[0])));
    for j in (2..coefficients.len()).step_by(2) {
        second = d.ar[j - 1].wrapping_add(mul_wb(d.ar[j].wrapping_sub(first), i32::from(warp)));
        d.ar[j - 1] = first;
        sum = sum.wrapping_add(mul_wb(first, i32::from(coefficients[j - 1])));
        first = d.ar[j].wrapping_add(mul_wb(d.ar[j + 1].wrapping_sub(second), i32::from(warp)));
        d.ar[j] = second;
        sum = sum.wrapping_add(mul_wb(second, i32::from(coefficients[j])));
    }
    d.ar[coefficients.len() - 1] = first;
    sum = sum.wrapping_add(mul_wb(
        first,
        i32::from(coefficients[coefficients.len() - 1]),
    ));
    sum.wrapping_shl(1)
        .wrapping_add(mul_wb(d.low, i32::from(tilt)))
        .wrapping_shl(2)
}
