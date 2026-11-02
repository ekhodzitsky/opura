//! Bounded little-endian RIFF/WAVE PCM16 I/O for the audio command.
use super::invalid;
use std::io::{self, Read, Seek, SeekFrom, Write};

pub struct Reader<R> {
    input: R,
    pub rate: u32,
    pub channels: usize,
    pub frames: u64,
    remaining: u64,
}
impl<R: Read + Seek> Reader<R> {
    pub fn new(mut input: R) -> io::Result<Self> {
        let length = input.seek(SeekFrom::End(0))?;
        input.seek(SeekFrom::Start(0))?;
        let mut head = [0; 12];
        input.read_exact(&mut head)?;
        if &head[..4] != b"RIFF" || &head[8..] != b"WAVE" {
            return Err(invalid("expected little-endian RIFF/WAVE"));
        }
        let end = 8 + u64::from(u32::from_le_bytes(
            head[4..8].try_into().map_err(|_| invalid("RIFF size"))?,
        ));
        if end < 12 || end > length {
            return Err(invalid("truncated RIFF/WAVE"));
        }
        let mut format = None;
        let mut data = None;
        let mut position = 12;
        while position < end {
            if end - position < 8 {
                return Err(invalid("truncated WAVE chunk header"));
            }
            input.seek(SeekFrom::Start(position))?;
            let mut chunk = [0; 8];
            input.read_exact(&mut chunk)?;
            let size = u64::from(u32::from_le_bytes(
                chunk[4..]
                    .try_into()
                    .map_err(|_| invalid("WAVE chunk size"))?,
            ));
            let next = position + 8 + size + (size & 1);
            if next > end {
                return Err(invalid("WAVE chunk exceeds RIFF size"));
            }
            match &chunk[..4] {
                b"fmt " => {
                    if format.is_some() || size < 16 {
                        return Err(invalid("missing/duplicate WAVE format"));
                    }
                    let mut f = [0; 16];
                    input.read_exact(&mut f)?;
                    let u16_at = |i| u16::from_le_bytes([f[i], f[i + 1]]);
                    let rate = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
                    let channels = usize::from(u16_at(2));
                    if u16_at(0) != 1 || u16_at(14) != 16 || !(1..=2).contains(&channels) {
                        return Err(invalid("WAVE input must be mono/stereo signed16 PCM"));
                    }
                    if ![8000, 12000, 16000, 24000, 48000].contains(&rate) {
                        return Err(invalid(
                            "WAVE input rate must be 8000/12000/16000/24000/48000 Hz",
                        ));
                    }
                    let alignment = channels as u16 * 2;
                    let byte_rate = u32::from_le_bytes([f[8], f[9], f[10], f[11]]);
                    if u16_at(12) != alignment || byte_rate != rate * u32::from(alignment) {
                        return Err(invalid("inconsistent WAVE block alignment/byte rate"));
                    }
                    format = Some((rate, channels));
                }
                b"data" => {
                    if data.is_some() {
                        return Err(invalid("multiple WAVE data chunks are unsupported"));
                    }
                    data = Some((position + 8, size));
                }
                _ => {}
            }
            position = next;
        }
        let (rate, channels) = format.ok_or_else(|| invalid("WAVE format chunk absent"))?;
        let (offset, remaining) = data.ok_or_else(|| invalid("WAVE data chunk absent"))?;
        if remaining % (channels as u64 * 2) != 0 {
            return Err(invalid("partial PCM sample/channel frame"));
        }
        input.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            input,
            rate,
            channels,
            frames: remaining / (channels as u64 * 2),
            remaining,
        })
    }
    pub fn read_samples(&mut self, output: &mut [i16]) -> io::Result<usize> {
        if output.len() > 1920 || output.len() % self.channels != 0 {
            return Err(invalid("invalid PCM read buffer"));
        }
        let bytes = (output.len() * 2).min(self.remaining as usize);
        let mut buffer = [0u8; 3840];
        self.input.read_exact(&mut buffer[..bytes])?;
        for (sample, pair) in output.iter_mut().zip(buffer[..bytes].chunks_exact(2)) {
            *sample = i16::from_le_bytes([pair[0], pair[1]]);
        }
        self.remaining -= bytes as u64;
        Ok(bytes / 2)
    }
}
pub struct Writer<W> {
    output: W,
    rate: u32,
    channels: usize,
    bytes: u32,
}
impl<W: Write + Seek> Writer<W> {
    pub fn new(output: W, rate: u32, channels: usize) -> io::Result<Self> {
        if !(1..=2).contains(&channels) || ![8000, 12000, 16000, 24000, 48000].contains(&rate) {
            return Err(invalid("invalid output WAVE geometry"));
        }
        let mut result = Self {
            output,
            rate,
            channels,
            bytes: 0,
        };
        result.header()?;
        Ok(result)
    }
    fn header(&mut self) -> io::Result<()> {
        let mut h = [0u8; 44];
        h[..4].copy_from_slice(b"RIFF");
        h[4..8].copy_from_slice(&(36 + self.bytes).to_le_bytes());
        h[8..16].copy_from_slice(b"WAVEfmt ");
        h[16..20].copy_from_slice(&16u32.to_le_bytes());
        h[20..22].copy_from_slice(&1u16.to_le_bytes());
        h[22..24].copy_from_slice(&(self.channels as u16).to_le_bytes());
        h[24..28].copy_from_slice(&self.rate.to_le_bytes());
        h[28..32].copy_from_slice(&(self.rate * self.channels as u32 * 2).to_le_bytes());
        h[32..34].copy_from_slice(&(self.channels as u16 * 2).to_le_bytes());
        h[34..36].copy_from_slice(&16u16.to_le_bytes());
        h[36..40].copy_from_slice(b"data");
        h[40..44].copy_from_slice(&self.bytes.to_le_bytes());
        self.output.seek(SeekFrom::Start(0))?;
        self.output.write_all(&h)
    }
    pub fn samples(&mut self, samples: &[i16]) -> io::Result<()> {
        if samples.len() % self.channels != 0 {
            return Err(invalid("partial output PCM channel frame"));
        }
        let bytes = u32::try_from(samples.len())
            .map_err(|_| invalid("PCM buffer too large"))?
            .checked_mul(2)
            .ok_or_else(|| invalid("PCM size overflow"))?;
        let total = self
            .bytes
            .checked_add(bytes)
            .filter(|&v| v <= u32::MAX - 36)
            .ok_or_else(|| invalid("WAVE output exceeds RIFF32 size limit"))?;
        let mut buffer = [0u8; 23040];
        for block in samples.chunks(11520) {
            for (sample, pair) in block.iter().zip(buffer.chunks_exact_mut(2)) {
                pair.copy_from_slice(&sample.to_le_bytes());
            }
            self.output.write_all(&buffer[..block.len() * 2])?;
        }
        self.bytes = total;
        Ok(())
    }
    pub fn finish(mut self) -> io::Result<()> {
        self.header()?;
        self.output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn pcm_extremes_unknown_chunks_and_truncation() -> io::Result<()> {
        let mut output = Cursor::new(Vec::new());
        let samples = [i16::MIN, -1, 0, 1, i16::MAX, 1234];
        let mut writer = Writer::new(&mut output, 16000, 2)?;
        writer.samples(&samples)?;
        writer.finish()?;
        let original = output.into_inner();
        assert_eq!(&original[44..48], &[0, 128, 255, 255]);
        for length in 0..original.len() {
            assert!(Reader::new(Cursor::new(&original[..length])).is_err());
        }
        let mut bytes = original.clone();
        bytes.splice(12..12, b"JUNK\x01\x00\x00\x00!\x00".iter().copied());
        let size = bytes.len() as u32 - 8;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let mut reader = Reader::new(Cursor::new(bytes))?;
        assert_eq!((reader.rate, reader.channels, reader.frames), (16000, 2, 3));
        let mut values = [0i16; 8];
        assert_eq!(reader.read_samples(&mut values)?, 6);
        assert_eq!(&values[..6], samples);
        assert_eq!(reader.read_samples(&mut values)?, 0);
        for (offset, value) in [(20, 3), (22, 3), (32, 1), (34, 24)] {
            let mut bad = original.clone();
            bad[offset] = value;
            assert!(Reader::new(Cursor::new(bad)).is_err());
        }
        let mut odd = original;
        odd[40..44].copy_from_slice(&11u32.to_le_bytes());
        assert!(Reader::new(Cursor::new(odd)).is_err());
        Ok(())
    }
    #[test]
    fn writer_size_error_does_not_write_more_pcm() -> io::Result<()> {
        let mut output = Cursor::new(Vec::new());
        let mut writer = Writer::new(&mut output, 48000, 1)?;
        writer.bytes = u32::MAX - 37;
        assert!(writer.samples(&[1]).is_err());
        assert_eq!(writer.bytes, u32::MAX - 37);
        assert_eq!(writer.output.get_ref().len(), 44);
        Ok(())
    }
}
