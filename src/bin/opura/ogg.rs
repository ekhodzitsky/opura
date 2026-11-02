//! Bounded Ogg version-zero pages, lacing and Opus packet assembly.
use super::invalid;
use std::io::{self, Read, Write};
const MAX_BODY: usize = 65025;
const MAX_PACKET: usize = 65536;
const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x80000000 != 0 {
                (value << 1) ^ 0x04c11db7
            } else {
                value << 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}
const CRC: [u32; 256] = crc_table();
pub fn crc(data: impl IntoIterator<Item = u8>) -> u32 {
    data.into_iter().fold(0, |sum, byte| {
        (sum << 8) ^ CRC[(((sum >> 24) ^ u32::from(byte)) & 255) as usize]
    })
}
pub struct Page {
    pub granule: Option<u64>,
    pub eos: bool,
    pub unfinished: bool,
    pub packets: Vec<Vec<u8>>,
}
pub struct Reader<R> {
    input: R,
    serial: Option<u32>,
    sequence: u32,
    pending: Vec<u8>,
    ended: bool,
}
impl<R: Read> Reader<R> {
    pub fn new(input: R) -> Self {
        Self {
            input,
            serial: None,
            sequence: 0,
            pending: Vec::new(),
            ended: false,
        }
    }
    pub fn page(&mut self) -> io::Result<Option<Page>> {
        let mut h = [0u8; 27];
        let count = self.input.read(&mut h[..1])?;
        if count == 0 {
            if !self.ended {
                return Err(invalid("Ogg stream lacks EOS"));
            }
            return Ok(None);
        }
        if self.ended {
            return Err(invalid("chained streams/trailing Ogg data are unsupported"));
        }
        self.input.read_exact(&mut h[1..])?;
        if &h[..4] != b"OggS" || h[4] != 0 || h[5] & !7 != 0 {
            return Err(invalid("invalid Ogg capture/version/flags"));
        }
        let flags = h[5];
        let granule = u64::from_le_bytes(h[6..14].try_into().map_err(|_| invalid("Ogg granule"))?);
        let serial = u32::from_le_bytes(h[14..18].try_into().map_err(|_| invalid("Ogg serial"))?);
        let sequence =
            u32::from_le_bytes(h[18..22].try_into().map_err(|_| invalid("Ogg sequence"))?);
        if sequence != self.sequence {
            return Err(invalid("Ogg page sequence discontinuity"));
        }
        match self.serial {
            None => {
                if flags & 2 == 0 {
                    return Err(invalid("Ogg stream lacks BOS"));
                }
                self.serial = Some(serial);
            }
            Some(previous) => {
                if previous != serial || flags & 2 != 0 {
                    return Err(invalid("multiplexed/chained Ogg streams are unsupported"));
                }
            }
        }
        if (flags & 1 != 0) == self.pending.is_empty() {
            return Err(invalid("inconsistent Ogg packet continuation"));
        }
        let mut laces = vec![0; usize::from(h[26])];
        self.input.read_exact(&mut laces)?;
        let length: usize = laces.iter().map(|&x| usize::from(x)).sum();
        let mut body = vec![0; length];
        self.input.read_exact(&mut body)?;
        let expected = u32::from_le_bytes(h[22..26].try_into().map_err(|_| invalid("Ogg CRC"))?);
        h[22..26].fill(0);
        if crc(h
            .into_iter()
            .chain(laces.iter().copied())
            .chain(body.iter().copied()))
            != expected
        {
            return Err(invalid("Ogg CRC mismatch"));
        }
        let mut packets = Vec::new();
        let mut offset = 0;
        for lace in laces {
            let end = offset + usize::from(lace);
            if self.pending.len() + usize::from(lace) > MAX_PACKET {
                return Err(invalid("Ogg packet exceeds 65536-byte command limit"));
            }
            self.pending.extend_from_slice(&body[offset..end]);
            offset = end;
            if lace < 255 {
                packets.push(std::mem::take(&mut self.pending));
            }
        }
        if packets.is_empty() && granule != u64::MAX {
            return Err(invalid("Ogg granule without completed packet"));
        }
        let eos = flags & 4 != 0;
        if eos && (!self.pending.is_empty() || packets.is_empty()) {
            return Err(invalid("incomplete Ogg EOS packet"));
        }
        self.ended = eos;
        self.sequence = self.sequence.wrapping_add(1);
        Ok(Some(Page {
            granule: if granule == u64::MAX {
                None
            } else {
                Some(granule)
            },
            eos,
            unfinished: !self.pending.is_empty(),
            packets,
        }))
    }
}
pub struct Writer<W> {
    output: W,
    sequence: u32,
    body: Vec<u8>,
    laces: Vec<u8>,
    granule: u64,
    last_page: u64,
}
impl<W: Write> Writer<W> {
    pub fn new(output: W, channels: usize, rate: u32) -> io::Result<Self> {
        if !(1..=2).contains(&channels) {
            return Err(invalid("invalid Opus channel count"));
        }
        let mut writer = Self {
            output,
            sequence: 0,
            body: Vec::with_capacity(MAX_BODY),
            laces: Vec::with_capacity(255),
            granule: 0,
            last_page: 0,
        };
        let mut head = Vec::from(&b"OpusHead"[..]);
        head.extend_from_slice(&[1, channels as u8]);
        head.extend_from_slice(&120u16.to_le_bytes());
        head.extend_from_slice(&rate.to_le_bytes());
        head.extend_from_slice(&[0, 0, 0]);
        writer.body.extend_from_slice(&head);
        writer.laces.push(19);
        writer.flush(2)?;
        let vendor = concat!("opura ", env!("CARGO_PKG_VERSION")).as_bytes();
        writer.body.extend_from_slice(b"OpusTags");
        writer
            .body
            .extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        writer.body.extend_from_slice(vendor);
        writer.body.extend_from_slice(&0u32.to_le_bytes());
        writer.laces.push(writer.body.len() as u8);
        writer.flush(0)?;
        Ok(writer)
    }
    fn flush(&mut self, flags: u8) -> io::Result<()> {
        let mut h = [0u8; 27];
        h[..4].copy_from_slice(b"OggS");
        h[5] = flags;
        h[6..14].copy_from_slice(&self.granule.to_le_bytes());
        h[14..18].copy_from_slice(&0x4f505552u32.to_le_bytes());
        h[18..22].copy_from_slice(&self.sequence.to_le_bytes());
        h[26] = self.laces.len() as u8;
        let sum = crc(h
            .into_iter()
            .chain(self.laces.iter().copied())
            .chain(self.body.iter().copied()));
        h[22..26].copy_from_slice(&sum.to_le_bytes());
        self.output.write_all(&h)?;
        self.output.write_all(&self.laces)?;
        self.output.write_all(&self.body)?;
        self.sequence = self.sequence.wrapping_add(1);
        self.last_page = self.granule;
        self.body.clear();
        self.laces.clear();
        Ok(())
    }
    pub fn packet(&mut self, packet: &[u8], granule: u64, eos: bool) -> io::Result<()> {
        let segments = packet.len() / 255 + 1;
        if packet.is_empty() || packet.len() > 1276 || granule < self.granule {
            return Err(invalid("invalid encoder Ogg packet/granule"));
        }
        if !self.laces.is_empty()
            && (self.laces.len() + segments > 255
                || self.body.len() + packet.len() > MAX_BODY
                || self.granule - self.last_page >= 48000)
        {
            self.flush(0)?;
        }
        self.laces
            .extend(std::iter::repeat_n(255, packet.len() / 255));
        self.laces.push((packet.len() % 255) as u8);
        self.body.extend_from_slice(packet);
        self.granule = granule;
        if eos {
            self.flush(4)?;
            self.output.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(flags: u8, sequence: u32, granule: u64, laces: &[u8], body: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; 27];
        h[..4].copy_from_slice(b"OggS");
        h[5] = flags;
        h[6..14].copy_from_slice(&granule.to_le_bytes());
        h[14..18].copy_from_slice(&7u32.to_le_bytes());
        h[18..22].copy_from_slice(&sequence.to_le_bytes());
        h[26] = laces.len() as u8;
        h.extend_from_slice(laces);
        h.extend_from_slice(body);
        let sum = crc(h.iter().copied());
        h[22..26].copy_from_slice(&sum.to_le_bytes());
        h
    }
    #[test]
    fn native_fixture_crc_and_page_boundaries() -> io::Result<()> {
        let bytes = include_bytes!("../../../tests/audio/celt-stereo.opus");
        let mut first = bytes[..47].to_vec();
        let expected = u32::from_le_bytes([first[22], first[23], first[24], first[25]]);
        first[22..26].fill(0);
        assert_eq!(crc(first), expected);
        let mut input = Reader::new(&bytes[..]);
        let mut count = 0;
        while let Some(page) = input.page()? {
            assert!(!page.unfinished);
            count += 1;
        }
        assert_eq!(count, 7);
        Ok(())
    }
    #[test]
    fn lacing_multiple_of_255_is_terminated() -> io::Result<()> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes, 2, 48000)?;
        let packet = [0x98; 255];
        writer.packet(&packet, 960, true)?;
        let mut input = Reader::new(bytes.as_slice());
        assert!(input.page()?.is_some());
        assert!(input.page()?.is_some());
        let page = input.page()?.ok_or_else(|| invalid("audio page absent"))?;
        assert_eq!(page.packets, [packet.to_vec()]);
        assert!(page.eos);
        assert!(input.page()?.is_none());
        Ok(())
    }
    #[test]
    fn continuations_and_packet_size_limit() -> io::Result<()> {
        let prefix = page(2, 0, u64::MAX, &[255; 255], &[1; 65025]);
        let mut exact = prefix.clone();
        exact.extend(page(5, 1, 1, &[255, 255, 1], &[2; 511]));
        let mut input = Reader::new(exact.as_slice());
        assert!(input.page()?.is_some());
        let complete = input
            .page()?
            .ok_or_else(|| invalid("continued page absent"))?;
        assert_eq!(complete.packets[0].len(), 65536);
        assert!(input.page()?.is_none());
        let mut over = prefix.clone();
        over.extend(page(5, 1, 1, &[255, 255, 2], &[2; 512]));
        let mut input = Reader::new(over.as_slice());
        input.page()?;
        assert!(input.page().is_err());
        let mut missing = prefix;
        missing.extend(page(4, 1, 1, &[1], &[2]));
        let mut input = Reader::new(missing.as_slice());
        input.page()?;
        assert!(input.page().is_err());
        assert!(
            Reader::new(page(3, 0, 0, &[1], &[0]).as_slice())
                .page()
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn corruption_missing_eos_sequence_and_trailing_data_reject() -> io::Result<()> {
        let good = page(6, 0, 0, &[1], &[0]);
        let mut corrupt = good.clone();
        corrupt[28] ^= 1;
        assert!(Reader::new(corrupt.as_slice()).page().is_err());
        let bytes = page(2, 0, 0, &[1], &[0]);
        let mut input = Reader::new(bytes.as_slice());
        input.page()?;
        assert!(input.page().is_err());
        let mut gap = bytes;
        gap.extend(page(4, 2, 1, &[1], &[0]));
        let mut input = Reader::new(gap.as_slice());
        input.page()?;
        assert!(input.page().is_err());
        let mut tail = good;
        tail.push(0);
        let mut input = Reader::new(tail.as_slice());
        input.page()?;
        assert!(input.page().is_err());
        Ok(())
    }
}
