use super::*;
use rustrtc::media::MediaKind;
use rustrtc::media::track::sample_track;
use std::path::Path;
use std::time::Instant;

fn status_kib(key: &str) -> u64 {
    let proc = std::fs::read_to_string("/proc/self/status").unwrap();
    proc.lines()
        .find(|l| l.starts_with(key))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}

fn ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let cols: Vec<_> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    cols[11].parse::<u64>().unwrap() + cols[12].parse::<u64>().unwrap()
}

pub async fn run(
    factory: impl Fn() -> Box<dyn Decoder>,
    legs: usize,
    directory: &Path,
    dump: Option<&Path>,
) {
    const WARM: usize = 12;
    const N: usize = 88;
    let corpus: Vec<Vec<Bytes>> = (0..15)
        .map(|i| {
            let bytes = std::fs::read(directory.join(format!("golos_{i:02}"))).unwrap();
            let mut rest = bytes.as_slice();
            let mut stream = Vec::new();
            while !rest.is_empty() {
                let n = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
                let frame = &rest[4..n + 4];
                assert_eq!(frame[0] & 4, 0, "this experiment covers mono speech");
                stream.push(Bytes::copy_from_slice(frame));
                rest = &rest[n + 4..];
            }
            assert!(stream.len() >= WARM + N);
            stream
        })
        .collect();
    let rss_before = status_kib("VmRSS:");
    let mut states = Vec::with_capacity(legs);
    for _ in 0..legs {
        let (source, peer, _feedback) = sample_track(MediaKind::Audio, 64);
        let (sender, out_track, _feedback2) = sample_track(MediaKind::Audio, 64);
        let codec = EgressCodec {
            codec: CodecType::PCMU,
            payload_type: 0,
            clock_rate: 8000,
            dtmf_payload_type: Some(101),
            comfort_noise: false,
            comfort_noise_level_db: -35.0,
        };
        let task = EgressTask {
            volume: None,
            volume_sink: Arc::new(parking_lot::Mutex::new(Default::default())),
            sender,
            codec,
            encoder: create_encoder(CodecType::PCMU),
            source: EgressSource::TranscodePeer {
                peer,
                decoder: factory(),
                source_audio_payload_type: 111,
                src_sample_rate: 48000,
                primed: false,
            },
            resampler: Some(BoxedResampler::new(48000, 8000).unwrap()),
            ptime: Duration::from_millis(20),
            gate: None,
            marker_pending: false,
            pcm_buf: vec![0; 160],
            noise_state: 0x9E37_79B9,
            noise_amplitude: 0.0,
            noise_lp: 0.0,
            eof_tail_total: 0,
            eof_tail_remaining: 0,
            transcode_pending: Vec::new(),
            transcode_overflow_samples: 0,
            transcode_armed: false,
        };
        states.push((source, task, out_track));
    }
    let mut checksum = 0xcbf29ce484222325u64;
    let mut sink = Vec::new();
    let mut latencies = Vec::new();
    let mut cpu_start = 0;
    let mut noise = 0usize;
    let mut start = Instant::now();
    for i in 0..WARM + N {
        if i == WARM {
            start = Instant::now();
            cpu_start = ticks();
        }
        let due = if i >= WARM {
            start + Duration::from_millis(((i - WARM) * 20) as u64)
        } else {
            Instant::now()
        };
        if i >= WARM {
            std::thread::sleep(due.saturating_duration_since(Instant::now()));
        }
        for (j, (source, task, _out)) in states.iter_mut().enumerate() {
            let data = corpus[j % 15][i].clone();
            source
                .try_send(MediaSample::Audio(AudioFrame {
                    data,
                    payload_type: Some(111),
                    rtp_timestamp: (i * 960) as u32,
                    clock_rate: 48000,
                    ..Default::default()
                }))
                .unwrap();
            let out = task.next_frame().await.expect("transcoded audio");
            assert_eq!(out.payload_type, Some(0));
            assert_eq!(out.data.len(), 160);
            if i >= WARM {
                if out.data.iter().all(|s| *s == 255 || *s == 127) {
                    noise += 1;
                }
                if dump.is_some() {
                    sink.extend_from_slice(&out.data);
                }
                for b in out.data {
                    checksum = (checksum ^ u64::from(b)).wrapping_mul(0x100000001b3);
                }
            }
        }
        if i >= WARM {
            latencies.push(due.elapsed().as_micros() as u64);
        }
    }
    let cpu_ticks = ticks() - cpu_start;
    let rss = status_kib("VmRSS:");
    let peak = status_kib("VmHWM:");
    if let Some(path) = dump {
        std::fs::write(path, sink).unwrap();
    }
    println!(
        "{}",
        serde_json::json!({"legs":legs,"cpu_ticks":cpu_ticks,"rss_kib":rss,"peak_kib":peak,"rss_before_kib":rss_before,"latency_us":latencies,"frames":N*legs,"silent_frames":noise,"fnv":format!("{checksum:016x}"),"ticks":N})
    );
}
