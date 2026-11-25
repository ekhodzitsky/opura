struct OpusDecoderRaw(opura::decoder::OpusDecoder);

impl OpusDecoderRaw {
    fn new(hz: i32, channels: usize) -> Result<Self, ()> {
        use opura::silk::{decoder::PcmChannels, resampler::OutputRate};
        let rate = match hz {
            8000 => OutputRate::Khz8,
            12000 => OutputRate::Khz12,
            16000 => OutputRate::Khz16,
            24000 => OutputRate::Khz24,
            48000 => OutputRate::Khz48,
            _ => return Err(()),
        };
        let channels = match channels {
            1 => PcmChannels::Mono,
            2 => PcmChannels::Stereo,
            _ => return Err(()),
        };
        opura::decoder::OpusDecoder::new(rate, channels)
            .map(Self)
            .map_err(|_| ())
    }
    fn decode(&mut self, packet: &[u8], max_frame: usize, out: &mut [f32]) -> Result<usize, ()> {
        let pcm = self.0.decode(packet).map_err(|_| ())?;
        if pcm.samples_per_channel() > max_frame || pcm.samples().len() > out.len() {
            return Err(());
        }
        for (o, s) in out.iter_mut().zip(pcm.samples()) {
            *o = f32::from(*s) / 32768.0;
        }
        Ok(pcm.samples_per_channel())
    }
}
