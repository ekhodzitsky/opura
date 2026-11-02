//! Bounded compatible-frame grouping and aggregate packet budgets for file I/O.
use super::{invalid, ogg};
use opura::{Packet, repacketizer::Repacketizer};
use std::io::{self, Write};
const DENOMINATOR: u64 = 48000 * 8;

pub struct Plan {
    frames: usize,
    frame_samples: u16,
    bitrate: u32,
    variable: bool,
    maximum: usize,
    component: usize,
}
impl Plan {
    pub fn new(
        frame_samples: u16,
        packet_samples: Option<u16>,
        bitrate: u32,
        variable: bool,
        maximum: Option<usize>,
    ) -> io::Result<Self> {
        let packet_samples = packet_samples.unwrap_or(frame_samples);
        if frame_samples == 0
            || packet_samples < frame_samples
            || packet_samples > 5760
            || packet_samples % frame_samples != 0
        {
            return Err(invalid("packet-ms must be a multiple of frame-ms up to120"));
        }
        let frames = usize::from(packet_samples / frame_samples);
        if frames > 48 {
            return Err(invalid("packet contains more than48 frames"));
        }
        let maximum = maximum.unwrap_or(1276 * frames);
        if !(3..=1276 * frames).contains(&maximum) {
            return Err(invalid("max-packet-bytes exceeds frame-count capacity"));
        }
        let total = u64::from(bitrate) * u64::from(packet_samples);
        if !variable && total.div_ceil(DENOMINATOR) > maximum as u64 {
            return Err(invalid("packet bitrate exceeds frame-count capacity"));
        }
        let component = if frames == 1 {
            maximum
        } else {
            // Same conservative worst-header reservation as native long-frame
            // encoding. Exact padding uses the actual emitted frame count.
            let header = if frames == 2 { 3 } else { 2 + 2 * (frames - 1) };
            let budget = if variable {
                maximum
            } else {
                (total / DENOMINATOR) as usize
            };
            budget.saturating_sub(header) / frames + 1
        };
        if !(3..=1276).contains(&component) {
            return Err(invalid("packet budget cannot fit its encoder frames"));
        }
        Ok(Self {
            frames,
            frame_samples,
            bitrate,
            variable,
            maximum,
            component,
        })
    }
    pub fn component_bytes(&self, legacy_bytes: usize) -> usize {
        if self.frames == 1 && !self.variable {
            legacy_bytes
        } else {
            self.component
        }
    }
}

pub struct Writer<W> {
    output: ogg::Writer<W>,
    plan: Plan,
    pending: Vec<Vec<u8>>,
    toc: Option<u8>,
    granule: u64,
    credit: u64,
    scratch: Vec<u8>,
    packets: u64,
    bytes: u64,
}
impl<W: Write> Writer<W> {
    pub fn new(output: ogg::Writer<W>, plan: Plan) -> Self {
        let frames = plan.frames;
        let maximum = plan.maximum;
        Self {
            output,
            plan,
            pending: Vec::with_capacity(frames),
            toc: None,
            granule: 0,
            credit: DENOMINATOR / 2,
            scratch: vec![0; maximum],
            packets: 0,
            bytes: 0,
        }
    }
    pub fn packet_count(&self) -> u64 {
        self.packets
    }
    pub fn packet_bytes(&self) -> u64 {
        self.bytes
    }
    pub fn component_bytes(&self, legacy_bytes: usize) -> usize {
        self.plan.component_bytes(legacy_bytes)
    }
    pub fn packet(&mut self, bytes: &[u8], granule: u64, eos: bool) -> io::Result<()> {
        if self.plan.frames == 1 {
            self.output.packet(bytes, granule, eos)?;
            self.packets += 1;
            self.bytes += bytes.len() as u64;
            return Ok(());
        }
        let packet = Packet::parse(bytes).map_err(|e| invalid(e.to_string()))?;
        if packet.duration_samples() != self.plan.frame_samples {
            return Err(invalid("encoded frame duration differs from packet plan"));
        }
        let toc = bytes[0] & 0xfc;
        if self.toc.is_some_and(|previous| previous != toc) {
            // Keep automatic bandwidth histories intact. Incompatible TOCs
            // close a shorter packet instead of mixing configurations.
            self.flush(false)?;
        }
        self.toc = Some(toc);
        self.pending.push(bytes.to_vec());
        self.granule = granule;
        if self.pending.len() == self.plan.frames || eos {
            self.flush(eos)?;
        }
        Ok(())
    }
    fn flush(&mut self, eos: bool) -> io::Result<()> {
        if self.pending.is_empty() {
            return Err(invalid("empty encoded packet group"));
        }
        let mut assembly = Repacketizer::new();
        for packet in &self.pending {
            assembly
                .add_packet(packet)
                .map_err(|e| invalid(e.to_string()))?;
        }
        let samples = u64::from(assembly.duration_samples());
        self.credit += u64::from(self.plan.bitrate) * samples;
        let target = (self.credit / DENOMINATOR) as usize;
        self.credit %= DENOMINATOR;
        let suppressed = self.pending.iter().all(|p| p.len() == 1);
        let length = if self.plan.variable || suppressed {
            assembly.write(&mut self.scratch)
        } else {
            assembly.write_padded(&mut self.scratch, target)
        }
        .map_err(|e| invalid(e.to_string()))?;
        self.output
            .packet(&self.scratch[..length], self.granule, eos)?;
        self.packets += 1;
        self.bytes += length as u64;
        self.pending.clear();
        self.toc = None;
        Ok(())
    }
}

pub fn duration(value: &str) -> io::Result<u16> {
    let (whole, half) = value
        .strip_suffix(".5")
        .map_or((value, false), |v| (v, true));
    let whole = whole
        .parse::<u16>()
        .map_err(|_| invalid("invalid packet-ms"))?;
    let ticks = whole
        .checked_mul(2)
        .and_then(|v| v.checked_add(u16::from(half)))
        .filter(|&v| (1..=240).contains(&v))
        .ok_or_else(|| invalid("packet-ms must be positive and at most120"))?;
    Ok(ticks * 24)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_changes_close_compatible_groups_and_keep_total_cbr_credit() -> io::Result<()> {
        let mut bytes = Vec::new();
        {
            let output = ogg::Writer::new(&mut bytes, 1, 48000)?;
            let plan = Plan::new(480, Some(2880), 24001, false, None)?;
            let mut writer = Writer::new(output, plan);
            for i in 0..6 {
                let packet = if i < 2 { &[0, 1, 2] } else { &[32, 3, 4] };
                writer.packet(packet, (i + 1) * 480, i == 5)?;
            }
            assert_eq!((writer.packet_count(), writer.packet_bytes()), (2, 180));
        }
        let mut reader = ogg::Reader::new(bytes.as_slice());
        let mut packets = Vec::new();
        while let Some(page) = reader.page()? {
            packets.extend(page.packets);
        }
        let first = Packet::parse(&packets[2]).map_err(|e| invalid(e.to_string()))?;
        let second = Packet::parse(&packets[3]).map_err(|e| invalid(e.to_string()))?;
        assert_eq!((first.configuration(), first.duration_samples()), (0, 960));
        assert_eq!(
            (second.configuration(), second.duration_samples()),
            (4, 1920)
        );
        assert_eq!((packets[2].len(), packets[3].len()), (60, 120));
        Ok(())
    }
}
