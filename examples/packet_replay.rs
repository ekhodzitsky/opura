//! Optional actual packet/loss/FEC replay fingerprints for native validation.
use opura::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::{
    env,
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = env::args_os().nth(1).ok_or("expected input vectors")?;
    let output = env::args_os().nth(2).ok_or("expected new result path")?;
    let mut input = BufReader::new(File::open(input)?);
    let mut output = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    if &magic != b"OPURARI1" {
        return Err("invalid replay format".into());
    }
    output.write_all(b"OPURARO1")?;
    let mut decoder = None;
    let mut channels = 0;
    loop {
        let mut header = [0; 6];
        if input.read(&mut header[..1])? == 0 {
            break;
        }
        input.read_exact(&mut header[1..])?;
        let flags = header[0];
        let actual_channels = header[1];
        let length = u16::from_le_bytes(header[2..4].try_into()?);
        let duration = u16::from_le_bytes(header[4..6].try_into()?);
        if flags & !3 != 0 || !(1..=2).contains(&actual_channels) || length > 61248 {
            return Err("invalid replay header".into());
        }
        if flags & 1 != 0 {
            decoder = Some(OpusDecoder::new(
                OutputRate::Khz48,
                if actual_channels == 1 {
                    PcmChannels::Mono
                } else {
                    PcmChannels::Stereo
                },
            )?);
            channels = actual_channels;
        }
        if channels != actual_channels {
            return Err("missing replay stream reset".into());
        }
        let mut packet = vec![0; usize::from(length)];
        input.read_exact(&mut packet)?;
        let decoder = decoder.as_mut().ok_or("missing replay stream")?;
        let pcm = if packet.is_empty() {
            decoder.conceal_samples(duration)?
        } else if flags & 2 != 0 {
            decoder.decode_fec(&packet, duration)?
        } else {
            decoder.decode(&packet)?
        };
        if pcm.samples().len() != usize::from(duration) * usize::from(channels) {
            return Err("replay duration mismatch".into());
        }
        let hash = pcm
            .samples()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
        output.write_all(&(pcm.samples().len() as u16).to_le_bytes())?;
        output.write_all(&pcm.final_range().to_le_bytes())?;
        output.write_all(&hash.to_le_bytes())?;
    }
    output.flush()?;
    Ok(())
}
