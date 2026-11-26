use super::*;

/// Decoder for many concurrent speech streams. Each stream retains CELT synthesis
/// history on the heap only after a packet or recovery operation needs it.
/// Use [`OpusDecoder`] when fixed storage is required throughout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactOpusDecoder {
    state: StreamState,
    celt: Option<Box<CeltDecoder>>,
    bands: (usize, usize),
}

impl CompactOpusDecoder {
    /// Construct a stream with no CELT synthesis history.
    pub fn new(rate: OutputRate, channels: PcmChannels) -> Result<Self, DecoderError> {
        Ok(Self {
            state: StreamState::new(rate, channels),
            celt: None,
            bands: (0, 21),
        })
    }

    /// Fixed PCM sampling rate.
    pub fn output_rate(&self) -> OutputRate {
        self.state.silk.output_rate()
    }

    /// Fixed PCM channel count.
    pub fn output_channels(&self) -> PcmChannels {
        self.state.silk.output_channels()
    }

    /// Output gain in signed Q8 decibels.
    pub fn gain_q8(&self) -> i16 {
        self.state.gain_q8
    }

    /// Set the same output gain used by [`OpusDecoder::set_gain_q8`].
    pub fn set_gain_q8(&mut self, gain: i16) {
        self.state.gain_q8 = gain;
    }

    /// Last successful operation's final range.
    pub fn final_range(&self) -> u32 {
        self.state.range
    }

    /// Last successful operation's duration on the 48 kHz clock.
    pub fn last_packet_duration(&self) -> u16 {
        self.state.last_duration
    }

    /// Last frame's mode, including concealment.
    pub fn previous_mode(&self) -> Option<Mode> {
        self.state.previous
    }

    /// Whether the previous frame ended with a SILK-to-CELT redundant tail.
    pub fn previous_redundancy(&self) -> bool {
        self.state.redundancy
    }

    /// Retained CELT synthesis diagnostics, once this stream has used CELT.
    pub fn celt(&self) -> Option<&CeltDecoder> {
        self.celt.as_deref()
    }

    /// Reset histories and release the CELT allocation. Keep gain and band limits.
    pub fn reset(&mut self) {
        let gain = self.gain_q8();
        self.state = StreamState::new(self.output_rate(), self.output_channels());
        self.state.gain_q8 = gain;
        self.celt = None;
    }

    fn staged(&self) -> Result<OpusDecoder, DecoderError> {
        let celt = match self.celt.as_deref() {
            Some(celt) => celt.clone(),
            None => {
                let mut celt = CeltDecoder::new(
                    decimation(self.output_rate()),
                    self.output_channels().count(),
                )?;
                celt.configure_bands(self.bands.0, self.bands.1)?;
                celt
            }
        };
        Ok(OpusDecoder {
            state: self.state.clone(),
            celt,
        })
    }

    fn commit(&mut self, next: OpusDecoder) {
        self.state = next.state;
        self.bands = (
            next.celt.concealment().start_band(),
            next.celt.concealment().end_band(),
        );
        if let Some(celt) = self.celt.as_deref_mut() {
            *celt = next.celt;
        } else if next.celt.has_synthesis_history() {
            self.celt = Some(Box::new(next.celt));
        }
    }

    /// Decode one complete packet, preserving state on error.
    pub fn decode(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
        let packet = Packet::parse(data).map_err(DecoderError::Framing)?;
        let mut next = self.staged()?;
        let pcm = next.decode_packet_staged(&packet)?;
        self.commit(next);
        Ok(pcm)
    }

    /// Decode into reusable PCM storage with the same error and suffix guarantees
    /// as [`OpusDecoder::decode_into`].
    pub fn decode_into(&mut self, data: &[u8], output: &mut [i16]) -> Result<usize, DecoderError> {
        let packet = Packet::parse(data).map_err(DecoderError::Framing)?;
        let required = usize::from(packet.duration_samples()) * self.output_rate().hz() as usize
            / 48000
            * self.output_channels().count();
        let out = output
            .get_mut(..required)
            .ok_or(DecoderError::BufferTooSmall { required })?;
        let mut next = self.staged()?;
        let count = next.decode_packet_into_staged(&packet, out)?;
        self.commit(next);
        Ok(count)
    }

    /// Conceal a missing interval, measured on the 48 kHz clock.
    pub fn conceal_samples(&mut self, duration: u16) -> Result<DecodedPcm, DecoderError> {
        let mut next = self.staged()?;
        let pcm = next.conceal_samples(duration)?;
        self.commit(next);
        Ok(pcm)
    }

    /// Recover a missing interval using FEC, or conceal it when FEC is absent.
    pub fn decode_fec(&mut self, data: &[u8], duration: u16) -> Result<DecodedPcm, DecoderError> {
        let mut next = self.staged()?;
        let pcm = next.decode_fec(data, duration)?;
        self.commit(next);
        Ok(pcm)
    }
}
