//! Bounded Ogg version-zero pages, lacing and Opus packet assembly.
use super::{invalid, layout::Layout};
use std::{
    hash::{BuildHasher, RandomState},
    io::{self, Read, Write},
    sync::{
        LazyLock,
        atomic::{AtomicU32, Ordering},
    },
};
const MAX_BODY: usize = 65025;
const MAX_PACKET: usize = opura::multistream::MAX_PACKET_BYTES;
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
                return Err(invalid("Ogg packet exceeds16MiB command limit"));
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
    serial: u32,
    sequence: u32,
    body: Vec<u8>,
    laces: Vec<u8>,
    granule: u64,
    last_page: u64,
    complete: bool,
    continued: bool,
    ended: bool,
}
impl<W: Write> Writer<W> {
    pub fn new(output: W, channels: usize, rate: u32) -> io::Result<Self> {
        Self::with_pre_skip(output, channels, rate, 120)
    }
    pub fn with_pre_skip(output: W, channels: usize, rate: u32, pre_skip: u16) -> io::Result<Self> {
        if !(1..=2).contains(&channels) {
            return Err(invalid("invalid Opus channel count"));
        }
        Self::with_layout(output, &Layout::positioned(channels)?, rate, pre_skip, 0)
    }
    pub fn with_layout(
        output: W,
        layout: &Layout,
        rate: u32,
        pre_skip: u16,
        gain: i16,
    ) -> io::Result<Self> {
        // Give independent encoders different starting points. Within this process
        // each logical stream gets a distinct serial until the u32 wraps.
        static NEXT_SERIAL: LazyLock<AtomicU32> =
            LazyLock::new(|| AtomicU32::new(RandomState::new().hash_one(()) as u32));
        let mut writer = Self {
            output,
            serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
            sequence: 0,
            body: Vec::with_capacity(MAX_BODY),
            laces: Vec::with_capacity(255),
            granule: 0,
            last_page: 0,
            complete: true,
            continued: false,
            ended: false,
        };
        let head = layout.header(rate, pre_skip, gain);
        writer.body.extend_from_slice(&head);
        writer.laces.push(head.len() as u8);
        writer.flush(2)?;
        let vendor = concat!("opura ", env!("CARGO_PKG_VERSION")).as_bytes();
        writer.body.extend_from_slice(b"OpusTags");
        writer
            .body
            .extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        writer.body.extend_from_slice(vendor);
        writer.body.extend_from_slice(&0u32.to_le_bytes());
        writer.laces.push(writer.body.len() as u8);
        writer.complete = true;
        writer.flush(0)?;
        Ok(writer)
    }
    fn flush(&mut self, flags: u8) -> io::Result<()> {
        let mut h = [0u8; 27];
        h[..4].copy_from_slice(b"OggS");
        h[5] = flags | u8::from(self.continued);
        h[6..14].copy_from_slice(
            &if self.complete {
                self.granule
            } else {
                u64::MAX
            }
            .to_le_bytes(),
        );
        h[14..18].copy_from_slice(&self.serial.to_le_bytes());
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
        if self.complete {
            self.last_page = self.granule;
        }
        self.body.clear();
        self.laces.clear();
        self.complete = false;
        self.continued = false;
        Ok(())
    }
    pub fn packet(&mut self, packet: &[u8], granule: u64, eos: bool) -> io::Result<()> {
        let segments = packet.len() / 255 + 1;
        if self.ended || packet.is_empty() || packet.len() > MAX_PACKET || granule < self.granule {
            return Err(invalid("invalid encoder Ogg packet/granule"));
        }
        if !self.laces.is_empty()
            && (self.laces.len() + segments > 255
                || self.body.len() + packet.len() > MAX_BODY
                || self.granule - self.last_page >= 48000)
        {
            self.flush(0)?;
        }
        let mut offset = 0;
        for segment in 0..segments {
            if self.laces.len() == 255 {
                self.flush(0)?;
                self.continued = segment != 0;
            }
            let length = (packet.len() - offset).min(255);
            self.laces.push(length as u8);
            self.body
                .extend_from_slice(&packet[offset..offset + length]);
            offset += length;
        }
        self.granule = granule;
        self.complete = true;
        if eos {
            self.flush(4)?;
            self.output.flush()?;
            self.ended = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_writers_keep_distinct_serials_through_eos() -> io::Result<()> {
        let stream_serial = || -> io::Result<Option<u32>> {
            let mut bytes = Vec::new();
            Writer::new(&mut bytes, 1, 48000)?.packet(&[0xf8, 0xff, 0xfe], 960, true)?;
            let mut reader = Reader::new(bytes.as_slice());
            let mut pages = 0;
            while reader.page()?.is_some() {
                pages += 1;
            }
            assert_eq!(pages, 3); // Identification, comments and audio.
            Ok(reader.serial)
        };
        assert_ne!(stream_serial()?, stream_serial()?);
        Ok(())
    }

    #[test]
    fn grouped_encoder_packets_fit_pages_and_preserve_lacing_crc_and_duration() -> io::Result<()> {
        let mut bytes = Vec::new();
        {
            let mut output = Writer::new(&mut bytes, 2, 48000)?;
            for (i, size) in [1275, 1276, 1277, 61248].into_iter().enumerate() {
                output.packet(&vec![0x55; size], (i as u64 + 1) * 5760, i == 3)?;
            }
        }
        let mut reader = Reader::new(bytes.as_slice());
        let mut sizes = Vec::new();
        let mut last = 0;
        while let Some(page) = reader.page()? {
            sizes.extend(page.packets.iter().map(Vec::len));
            last = page.granule.unwrap_or(last);
        }
        assert_eq!(
            sizes,
            vec![
                19,
                16 + concat!("opura ", env!("CARGO_PKG_VERSION")).len(),
                1275,
                1276,
                1277,
                61248
            ]
        );
        assert_eq!(last, 23040);
        Ok(())
    }
    #[test]
    fn mapped_headers_and_multi_page_packets_retain_crc_granules_and_eos() -> io::Result<()> {
        let mut bytes = Vec::new();
        let sizes = [65024, 65025, 65026, 90000];
        {
            let layout = Layout::positioned(8)?;
            let mut writer = Writer::with_layout(&mut bytes, &layout, 48000, 312, 1536)?;
            for (i, size) in sizes.into_iter().enumerate() {
                writer.packet(&vec![i as u8 + 1; size], (i as u64 + 1) * 960, i == 3)?;
            }
            let before = writer.output.len();
            assert!(writer.packet(&[1], 4000, false).is_err());
            assert_eq!(writer.output.len(), before);
        }
        let mut reader = Reader::new(bytes.as_slice());
        let first = reader.page()?.ok_or_else(|| invalid("header absent"))?;
        let header = super::super::layout::parse(&first.packets[0])?;
        assert_eq!(header.layout.channels(), 8);
        assert_eq!(header.layout.family(), 1);
        assert_eq!(header.gain_q8, 1536);
        assert_eq!(header.skip, 312);
        reader.page()?;
        let mut index = 0;
        let mut unfinished = 0;
        let mut end = false;
        while let Some(page) = reader.page()? {
            unfinished += usize::from(page.unfinished);
            for packet in &page.packets {
                assert_eq!(packet.len(), sizes[index]);
                assert!(packet.iter().all(|&b| b == index as u8 + 1));
                index += 1;
            }
            if let Some(g) = page.granule {
                assert_eq!(g, index as u64 * 960);
            }
            end = page.eos;
        }
        assert_eq!(index, 4);
        assert!(unfinished >= 3);
        assert!(end);
        Ok(())
    }
    #[test]
    fn full_packet_limit_accepts_exact_and_rejects_excess_before_publish() -> io::Result<()> {
        let mut bytes = Vec::new();
        {
            let mut writer = Writer::new(&mut bytes, 1, 48000)?;
            let before = writer.output.len();
            assert!(writer.packet(&vec![0; MAX_PACKET + 1], 960, false).is_err());
            assert_eq!(writer.output.len(), before);
            writer.packet(&vec![7; MAX_PACKET], 960, true)?;
        }
        let mut reader = Reader::new(bytes.as_slice());
        reader.page()?;
        reader.page()?;
        let mut found = 0;
        while let Some(page) = reader.page()? {
            for packet in page.packets {
                assert_eq!(packet.len(), MAX_PACKET);
                assert!(packet.iter().all(|&b| b == 7));
                found += 1;
            }
        }
        assert_eq!(found, 1);
        let mut over = Vec::new();
        let full = (MAX_PACKET + 1) / MAX_BODY;
        for index in 0..full {
            over.extend(page(
                if index == 0 { 2 } else { 1 },
                index as u32,
                u64::MAX,
                &[255; 255],
                &vec![1; MAX_BODY],
            ));
        }
        let remaining = (MAX_PACKET + 1) % MAX_BODY;
        let mut laces = vec![255; remaining / 255];
        laces.push((remaining % 255) as u8);
        over.extend(page(5, full as u32, 960, &laces, &vec![1; remaining]));
        let mut reader = Reader::new(over.as_slice());
        for _ in 0..full {
            reader.page()?;
        }
        assert!(reader.page().is_err());
        Ok(())
    }
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
        assert_eq!(
            input
                .page()?
                .ok_or_else(|| invalid("continued page absent"))?
                .packets[0]
                .len(),
            65537
        );
        assert!(input.page()?.is_none());
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
