//! Native TF differential signalling and spreading selection.
use super::*;
use crate::range::RangeEncoder;

fn validate(entropy: &RangeEncoder) -> Result<u32, ControlError> {
    let budget = entropy.capacity() as u32 * 8;
    if !(16..=10200).contains(&budget) {
        return Err(ControlError::InvalidPayload);
    }
    if entropy.tell() > budget {
        return Err(ControlError::BitBudgetExceeded);
    }
    if entropy.is_finished() {
        return Err(EncoderError::Finished.into());
    }
    Ok(budget)
}
/// Encode binary per-band TF analysis decisions and return effective resolutions.
///
/// Selected bands require start<=end<=21, with zero result outside that range.
/// `select` chooses the alternate native mapping when a reserved field can affect
/// it. Decisions that cannot fit retain the current differential state. LM0
/// cannot be transient. Physical capacity is 2..1275 bytes and cursor must fit.
/// Caller decisions remain immutable; every error preserves the whole writer.
pub fn encode_tf(
    entropy: &mut RangeEncoder,
    size: FrameSize,
    bands: Range<usize>,
    transient: bool,
    decisions: &[bool; 21],
    select: bool,
) -> Result<TfResolution, ControlError> {
    encode_tf_with(
        entropy,
        size,
        bands,
        transient,
        decisions,
        select,
        RangeEncoder::encode_bit_logp,
    )
}
// Static injection keeps symbol failure verification on the complete transaction.
fn encode_tf_with(
    entropy: &mut RangeEncoder,
    size: FrameSize,
    bands: Range<usize>,
    transient: bool,
    decisions: &[bool; 21],
    select: bool,
    mut write_bit: impl FnMut(&mut RangeEncoder, bool, u32) -> Result<(), EncoderError>,
) -> Result<TfResolution, ControlError> {
    if bands.start > bands.end || bands.end > 21 {
        return Err(ControlError::InvalidBands);
    }
    if size == FrameSize::Ms2_5 && transient {
        return Err(ControlError::InvalidContext);
    }
    let mut budget = validate(entropy)?;
    let mut writer = entropy.clone();
    let mut tell = writer.tell();
    let mut logp = if transient { 2 } else { 4 };
    let reserved = size != FrameSize::Ms2_5 && tell + logp < budget;
    budget -= u32::from(reserved);
    let mut current = false;
    let mut changed = false;
    let mut result = TfResolution { values: [0; 21] };
    for i in bands.clone() {
        if tell + logp <= budget {
            write_bit(&mut writer, decisions[i] ^ current, logp)?;
            tell = writer.tell();
            current = decisions[i];
            changed |= current;
        }
        result.values[i] = i8::from(current);
        logp = if transient { 4 } else { 5 };
    }
    let base = 4 * usize::from(transient);
    let table = TF_SELECT[size.lm()];
    let effective_select = if reserved
        && table[base + usize::from(changed)] != table[base + 2 + usize::from(changed)]
    {
        write_bit(&mut writer, select, 1)?;
        select
    } else {
        false
    };
    for i in bands {
        result.values[i] =
            table[base + 2 * usize::from(effective_select) + result.values[i] as usize];
    }
    *entropy = writer;
    Ok(result)
}
/// Encode spreading when at least four whole physical bits remain.
/// Otherwise return Normal without writing. Capacity/lifecycle errors preserve
/// the writer, including a finished writer in the no-symbol fallback case.
pub fn encode_spread(
    entropy: &mut RangeEncoder,
    requested: Spread,
) -> Result<Spread, ControlError> {
    let budget = validate(entropy)?;
    if entropy.tell() + 4 > budget {
        return Ok(Spread::Normal);
    }
    let index = match requested {
        Spread::None => 0,
        Spread::Light => 1,
        Spread::Normal => 2,
        Spread::Aggressive => 3,
    };
    entropy.encode_icdf(index, &SPREAD, 5)?;
    Ok(requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_at_first_middle_and_select_symbol_preserve_complete_writer()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = RangeEncoder::new(128)?;
        writer.encode_uint(13, 17)?;
        let original = writer.clone();
        let decisions = std::array::from_fn(|i| i % 2 == 0);
        for fail_at in [1, 2, 12, 22] {
            let mut count = 0;
            assert_eq!(
                encode_tf_with(
                    &mut writer,
                    FrameSize::Ms20,
                    0..21,
                    true,
                    &decisions,
                    true,
                    |staged, value, logp| {
                        count += 1;
                        if count == fail_at {
                            if count > 1 {
                                assert_ne!(staged, &original);
                            }
                            Err(EncoderError::CapacityExhausted)
                        } else {
                            staged.encode_bit_logp(value, logp)
                        }
                    }
                ),
                Err(ControlError::Encoding(EncoderError::CapacityExhausted))
            );
            assert_eq!(count, fail_at);
            assert_eq!(writer, original);
        }
        let mut count = 0;
        let result = encode_tf_with(
            &mut writer,
            FrameSize::Ms20,
            0..21,
            true,
            &decisions,
            true,
            |writer, value, logp| {
                count += 1;
                writer.encode_bit_logp(value, logp)
            },
        )?;
        assert_eq!(count, 22);
        let mut reference = original;
        assert_eq!(
            encode_tf(
                &mut reference,
                FrameSize::Ms20,
                0..21,
                true,
                &decisions,
                true
            )?,
            result
        );
        assert_eq!(writer, reference);
        Ok(())
    }
}
