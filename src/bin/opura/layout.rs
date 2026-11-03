//! OpusHead mapping metadata and explicit Vorbis/WAVE channel permutations.
//! Canonical mappings follow pinned Opus1.6.1 vorbis_mappings; copyright (c)
//! 2011 Xiph.Org Foundation, written by Jean-Marc Valin. See THIRD_PARTY_NOTICES.md.
use super::invalid;
use opura::multistream::ChannelMapping;
use std::io;

#[derive(Clone, Debug)]
pub struct Layout {
    family: u8,
    opus: ChannelMapping,
    wave: ChannelMapping,
}
impl Layout {
    fn mapped(family: u8, opus: ChannelMapping) -> io::Result<Self> {
        let n = opus.channels();
        if !(1..=8).contains(&n) {
            return Err(invalid("file layouts require1..8channels"));
        }
        let wave = if family == 1 {
            let order: &[usize] = match n {
                1 => &[0],
                2 => &[0, 1],
                3 => &[0, 2, 1],
                4 => &[0, 1, 2, 3],
                5 => &[0, 2, 1, 3, 4],
                6 => &[0, 2, 1, 5, 3, 4],
                7 => &[0, 2, 1, 6, 5, 3, 4],
                8 => &[0, 2, 1, 7, 5, 6, 3, 4],
                _ => return Err(invalid("unsupported positioned channel order")),
            };
            let mapping: Vec<u8> = order.iter().map(|&i| opus.mapping()[i]).collect();
            ChannelMapping::new(opus.streams(), opus.coupled_streams(), &mapping)
                .map_err(|e| invalid(e.to_string()))?
        } else {
            opus.clone()
        };
        Ok(Self { family, opus, wave })
    }
    pub fn positioned(channels: usize) -> io::Result<Self> {
        let (streams, coupled, map): (usize, usize, &[u8]) = match channels {
            1 => (1, 0, &[0]),
            2 => (1, 1, &[0, 1]),
            3 => (2, 1, &[0, 2, 1]),
            4 => (2, 2, &[0, 1, 2, 3]),
            5 => (3, 2, &[0, 4, 1, 2, 3]),
            6 => (4, 2, &[0, 4, 1, 2, 3, 5]),
            7 => (4, 3, &[0, 4, 1, 2, 3, 5, 6]),
            8 => (5, 3, &[0, 6, 1, 2, 3, 4, 5, 7]),
            _ => return Err(invalid("file layouts require1..8channels")),
        };
        let map = ChannelMapping::new(streams, coupled, map).map_err(|e| invalid(e.to_string()))?;
        Self::mapped(if channels <= 2 { 0 } else { 1 }, map)
    }
    pub fn unpositioned(channels: usize) -> io::Result<Self> {
        if !(1..=8).contains(&channels) {
            return Err(invalid("file layouts require1..8channels"));
        }
        let mapping: Vec<u8> = (0..channels as u8).collect();
        let map = ChannelMapping::new(channels, 0, &mapping).map_err(|e| invalid(e.to_string()))?;
        Self::mapped(255, map)
    }
    pub fn channels(&self) -> usize {
        self.opus.channels()
    }
    pub fn family(&self) -> u8 {
        self.family
    }
    pub fn positioned_order(&self) -> bool {
        self.family != 255
    }
    pub fn opus(&self) -> &ChannelMapping {
        &self.opus
    }
    pub fn wave(&self) -> &ChannelMapping {
        &self.wave
    }
    pub fn header(&self, rate: u32, skip: u16, gain: i16) -> Vec<u8> {
        let mut bytes = Vec::from(&b"OpusHead"[..]);
        bytes.extend([1, self.channels() as u8]);
        bytes.extend(skip.to_le_bytes());
        bytes.extend(rate.to_le_bytes());
        bytes.extend(gain.to_le_bytes());
        bytes.push(self.family);
        if self.family != 0 {
            bytes.extend([self.opus.streams() as u8, self.opus.coupled_streams() as u8]);
            bytes.extend_from_slice(self.opus.mapping());
        }
        bytes
    }
}
pub struct Header {
    pub layout: Layout,
    pub skip: u64,
    pub gain_q8: i16,
}
pub fn parse(packet: &[u8]) -> io::Result<Header> {
    if packet.len() < 19 || &packet[..8] != b"OpusHead" || packet[8] > 15 {
        return Err(invalid("invalid/unsupported OpusHead"));
    }
    let channels = usize::from(packet[9]);
    if !(1..=8).contains(&channels) {
        return Err(invalid("OpusHead file channels must be1..8"));
    }
    let family = packet[18];
    let layout = match family {
        0 if channels <= 2 => Layout::positioned(channels)?,
        1 | 255 => {
            let data = packet
                .get(19..21 + channels)
                .ok_or_else(|| invalid("truncated OpusHead mapping"))?;
            let map = ChannelMapping::new(usize::from(data[0]), usize::from(data[1]), &data[2..])
                .map_err(|e| invalid(format!("invalid OpusHead map: {e}")))?;
            Layout::mapped(family, map)?
        }
        _ => return Err(invalid("unsupported OpusHead mapping family/channel count")),
    };
    Ok(Header {
        layout,
        skip: u64::from(u16::from_le_bytes([packet[10], packet[11]])),
        gain_q8: i16::from_le_bytes([packet[16], packet[17]]),
    })
}
/// Canonical WAVE speaker masks: mono, stereo, LCR, quad,5.0,5.1,6.1,7.1.
pub fn wave_mask(channels: usize) -> io::Result<u32> {
    Ok(match channels {
        1 => 4,
        2 => 3,
        3 => 7,
        4 => 0x33,
        5 => 0x37,
        6 => 0x3f,
        7 => 0x70f,
        8 => 0x63f,
        _ => return Err(invalid("unsupported WAVE speaker count")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_headers_and_wave_permutations_preserve_each_coded_channel() -> io::Result<()> {
        let expected: [&[u8]; 8] = [
            &[0],
            &[0, 1],
            &[0, 1, 2],
            &[0, 1, 2, 3],
            &[0, 1, 4, 2, 3],
            &[0, 1, 4, 5, 2, 3],
            &[0, 1, 4, 6, 5, 2, 3],
            &[0, 1, 6, 7, 4, 5, 2, 3],
        ];
        for n in 1..=8 {
            let layout = Layout::positioned(n)?;
            assert_eq!(layout.wave().mapping(), expected[n - 1]);
            layout
                .wave()
                .validate_encoder()
                .map_err(|e| invalid(e.to_string()))?;
            let head = layout.header(48000, 312, -1536);
            let parsed = parse(&head)?;
            assert_eq!(parsed.layout.opus(), layout.opus());
            assert_eq!(parsed.layout.wave(), layout.wave());
            assert_eq!(parsed.gain_q8, -1536);
            assert_eq!(parsed.skip, 312);
            for length in 0..head.len() {
                assert!(parse(&head[..length]).is_err());
            }
            assert_eq!(wave_mask(n)?.count_ones() as usize, n);
            let unknown = Layout::unpositioned(n)?;
            let parsed = parse(&unknown.header(0, 0, i16::MIN))?;
            assert_eq!(parsed.layout.family(), 255);
            assert!(!parsed.layout.positioned_order());
            assert_eq!(parsed.layout.opus(), parsed.layout.wave());
        }
        Ok(())
    }
    #[test]
    fn malformed_counts_maps_and_unknown_families_reject() -> io::Result<()> {
        let valid = Layout::positioned(6)?.header(48000, 312, 0);
        for (index, value) in [
            (8, 16),
            (9, 0),
            (9, 9),
            (18, 2),
            (19, 0),
            (20, 5),
            (21, 254),
        ] {
            let mut bad = valid.clone();
            bad[index] = value;
            assert!(parse(&bad).is_err());
        }
        let mut silent = valid;
        silent[21..].fill(255);
        assert!(parse(&silent).is_ok());
        let mut zero = Layout::positioned(2)?.header(48000, 120, 0);
        zero[9] = 3;
        assert!(parse(&zero).is_err());
        Ok(())
    }
}
