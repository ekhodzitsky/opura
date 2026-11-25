use audio_codec::{CodecError, CodecType, Decoder};
use opura::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::path::Path;

struct PilotDecoder(OpusDecoder);
impl Decoder for PilotDecoder {
    fn sample_rate(&self) -> u32 {
        48000
    }
    fn channels(&self) -> u16 {
        1
    }
    fn max_decode_samples(&self, _: usize) -> usize {
        5760
    }
    fn decode_into(&mut self, data: &[u8], out: &mut [i16]) -> Result<usize, CodecError> {
        let pcm = self.0.decode(data).map_err(|_| CodecError::DecodeFailed)?;
        let n = pcm.samples().len();
        out.get_mut(..n)
            .ok_or(CodecError::BufferTooSmall)?
            .copy_from_slice(pcm.samples());
        Ok(n)
    }
    fn decode(&mut self, data: &[u8]) -> Vec<i16> {
        self.0
            .decode(data)
            .expect("valid audited speech packet")
            .samples()
            .to_vec()
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let argv = std::env::args().collect::<Vec<_>>();
    let factory = || -> Box<dyn Decoder> {
        match argv[1].as_str() {
            "stock" => audio_codec::create_decoder(CodecType::Opus),
            "opura" => Box::new(PilotDecoder(
                OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono).unwrap(),
            )),
            _ => panic!("arm"),
        }
    };
    rustpbx_media::egress::opura_probe::run(
        factory,
        argv[2].parse().unwrap(),
        Path::new(&argv[3]),
        argv.get(4).map(Path::new),
    )
    .await;
}
