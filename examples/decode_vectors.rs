//! Optional RFC8251 vector-file decoder; no external library is linked.
//! Usage: decode_vectors RATE CHANNELS INPUT.bit OUTPUT.pcm
//! Output is signed16 little-endian. Existing files are never overwritten;
//! failed decoding may leave a partial output, which must not be used.
use opura::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::{
    env,
    error::Error,
    fs::{File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::PathBuf,
};
const MAX_PACKET: usize = 65536;
const MAX_PACKETS: u64 = 1_000_000;
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
#[derive(Debug, Eq, PartialEq)]
struct Stats {
    packets: u64,
    range_checks: u64,
    samples_per_channel: u64,
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn read_record<'a>(
    input: &mut impl Read,
    buffer: &'a mut [u8; MAX_PACKET],
) -> io::Result<Option<(&'a [u8], u32)>> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length[..1]) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    input.read_exact(&mut length[1..])?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_PACKET {
        return Err(invalid("vector packet exceeds65536 bytes"));
    }
    let mut range = [0u8; 4];
    input.read_exact(&mut range)?;
    input.read_exact(&mut buffer[..length])?;
    Ok(Some((&buffer[..length], u32::from_be_bytes(range))))
}
fn decode(
    input: &mut impl Read,
    output: &mut impl Write,
    rate: OutputRate,
    channels: PcmChannels,
) -> Result<Stats, Box<dyn Error>> {
    let mut decoder = OpusDecoder::new(rate, channels)?;
    let mut packet = [0u8; MAX_PACKET];
    let mut bytes = [0u8; 23040];
    let mut stats = Stats {
        packets: 0,
        range_checks: 0,
        samples_per_channel: 0,
    };
    let mut consumed = 0u64;
    while let Some((data, expected_range)) = read_record(input, &mut packet)? {
        if stats.packets >= MAX_PACKETS {
            return Err(invalid("vector packet count exceeds1000000").into());
        }
        consumed += data.len() as u64 + 8;
        if consumed > MAX_INPUT_BYTES {
            return Err(invalid("vector input exceeds256MiB").into());
        }
        let result = if data.is_empty() {
            // A zero-length vector record has no TOC. Use the previous packet
            // duration (2.5ms before any packet) for explicit loss.
            decoder.conceal_samples(decoder.last_packet_duration().max(120))
        } else {
            decoder.decode(data)
        };
        let pcm = result.map_err(|e| invalid(format!("packet{}: {e}", stats.packets)))?;
        if expected_range != 0 {
            if pcm.final_range() != expected_range {
                return Err(invalid(format!(
                    "packet{} range mismatch: expected {expected_range:08x}, got {:08x}",
                    stats.packets,
                    pcm.final_range()
                ))
                .into());
            }
            stats.range_checks += 1;
        }
        for (target, &sample) in bytes.chunks_exact_mut(2).zip(pcm.samples()) {
            target.copy_from_slice(&sample.to_le_bytes());
        }
        output.write_all(&bytes[..pcm.samples().len() * 2])?;
        stats.packets += 1;
        stats.samples_per_channel += pcm.samples_per_channel() as u64;
    }
    if stats.packets == 0 {
        return Err(invalid("vector has no records").into());
    }
    output.flush()?;
    Ok(stats)
}
fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    if arguments.len() != 4 {
        return Err("usage: decode_vectors RATE CHANNELS INPUT.bit OUTPUT.pcm".into());
    }
    let rate = match arguments[0].to_str() {
        Some("8000") => OutputRate::Khz8,
        Some("12000") => OutputRate::Khz12,
        Some("16000") => OutputRate::Khz16,
        Some("24000") => OutputRate::Khz24,
        Some("48000") => OutputRate::Khz48,
        _ => return Err("unsupported output rate".into()),
    };
    let channels = match arguments[1].to_str() {
        Some("1") => PcmChannels::Mono,
        Some("2") => PcmChannels::Stereo,
        _ => return Err("channels must be1 or2".into()),
    };
    let input = File::open(PathBuf::from(&arguments[2]))?;
    if input.metadata()?.len() > MAX_INPUT_BYTES {
        return Err(invalid("vector input exceeds256MiB").into());
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(PathBuf::from(&arguments[3]))?;
    let stats = decode(
        &mut BufReader::new(input),
        &mut BufWriter::new(output),
        rate,
        channels,
    )?;
    println!(
        "{{\"packets\":{},\"range_checks\":{},\"samples_per_channel\":{}}}",
        stats.packets, stats.range_checks, stats.samples_per_channel
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    // First original public CELT fixture:8kHz mono2.5ms silence, range01000000.
    const RECORD: [u8; 16] = [0, 0, 0, 8, 1, 0, 0, 0, 0xe0, 0xff, 0xfe, 0, 0, 0, 0, 0];
    #[test]
    fn original_silence_record_checks_endianness_pcm_and_diagnostics() -> Result<(), Box<dyn Error>>
    {
        for channels in [PcmChannels::Mono, PcmChannels::Stereo] {
            let mut output = Vec::new();
            let stats = decode(
                &mut RECORD.as_slice(),
                &mut output,
                OutputRate::Khz8,
                channels,
            )?;
            assert_eq!(
                stats,
                Stats {
                    packets: 1,
                    range_checks: 1,
                    samples_per_channel: 20
                }
            );
            assert_eq!(output, vec![0; 40 * channels.count()]);
        }
        Ok(())
    }
    #[test]
    fn truncation_size_range_and_empty_input_fail_without_pcm() {
        let mut cases = vec![
            vec![],
            vec![0],
            vec![0, 0, 0],
            vec![0, 1, 0, 1],
            RECORD[..7].to_vec(),
            RECORD[..15].to_vec(),
        ];
        let mut wrong = RECORD.to_vec();
        wrong[4..8].copy_from_slice(&1u32.to_be_bytes());
        cases.push(wrong);
        for data in cases {
            let mut output = Vec::new();
            assert!(
                decode(
                    &mut data.as_slice(),
                    &mut output,
                    OutputRate::Khz8,
                    PcmChannels::Mono
                )
                .is_err()
            );
            assert!(output.is_empty());
        }
    }
    #[test]
    fn absent_range_check_is_reported_and_output_errors_propagate() -> Result<(), Box<dyn Error>> {
        let mut data = RECORD;
        data[4..8].fill(0);
        let mut output = Vec::new();
        assert_eq!(
            decode(
                &mut data.as_slice(),
                &mut output,
                OutputRate::Khz8,
                PcmChannels::Mono
            )?
            .range_checks,
            0
        );
        struct FailedWriter;
        impl Write for FailedWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("injected output failure"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert!(
            decode(
                &mut RECORD.as_slice(),
                &mut FailedWriter,
                OutputRate::Khz8,
                PcmChannels::Mono
            )
            .is_err()
        );
        struct FailedFlush;
        impl Write for FailedFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("injected flush failure"))
            }
        }
        assert!(
            decode(
                &mut RECORD.as_slice(),
                &mut FailedFlush,
                OutputRate::Khz8,
                PcmChannels::Mono
            )
            .is_err()
        );
        Ok(())
    }
}
