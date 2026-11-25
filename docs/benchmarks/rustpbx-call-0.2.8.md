# RustPBX SIP/RTP call smoke with Opura 0.2.8

Eight simultaneous 10-second calls completed through a local RustPBX 0.5.0 process with each decoder. The Opura build used 980 KiB less additional resident memory above idle across the eight calls and used more CPU during the measured interval. This was one paired run on one host. [Server CPU and memory samples](results/rustpbx-call-0.2.8.json)

## Calls and audio

A sipbot caller registered over SIP and sent Opus over RTP. RustPBX decoded Opus, converted the audio to PCMU and delivered it to a second registered sipbot process. The PCMU endpoint echoed the audio back through RustPBX, which re-encoded it with its stock Opus encoder. Both RTP directions used plain UDP on an isolated loopback network. Each backend received identical input. [RustPBX bridge](https://github.com/restsend/rustpbx/blob/bd3da23fa7af049dae7e150dae3330d3f1457fd4/crates/rustpbx-media/src/media_bridge.rs#L701-L736)

In a separate single-call check, the caller sent a 440 Hz tone. Both endpoints recorded their incoming audio in the left channel of a 16 kHz WAV file. For both decoders, all 40 complete 250 ms windows on each side contained the tone. The detector required RMS above 500 signed 16-bit units and more than half the energy at 440 Hz. This verifies that audio traveled in both directions. [Endpoint recording format](https://github.com/miuda-ai/sipbot/blob/74ba9e7a077155da277b1fe9bcb8455d75a1395e/src/recorder.rs)

The eight-call case started four calls per second and looped RustPBX's 5.44-second Mandarin speech fixture. In 67 of its 272 non-overlapping 20 ms windows the input RMS exceeded 500. RustPBX reported eight Opus-to-PCMU bridge activations per backend. The caller sent 4,000 RTP packets and received 3,992. The echo endpoint received and returned 3,992. Both backends produced those counts and both endpoints reported 0.00% average loss. [Speech fixture](https://github.com/restsend/rustpbx/blob/339f75b99341bcfc440545d70294f5de55652525/fixtures/hello_book_course_zh_16k.wav)

| RustPBX process metric | Stock opus-rs 0.1.32 | Opura 0.2.8 |
| --- | ---: | ---: |
| CPU, equivalents of one logical core | 0.154 | 0.173 |
| Idle RSS, KiB | 37,784 | 37,416 |
| Live median RSS, KiB | 53,072 | 51,724 |
| Live RSS above idle, KiB | 15,288 | 14,308 |
| Live median private dirty memory, KiB | 50,132 | 48,744 |
| Live private dirty memory above idle, KiB | 15,160 | 14,124 |

The endpoint audio detectors also differed. On returning Opus the caller flagged 64 clipped frames with stock and 72 with Opura out of 3,992. On incoming PCMU the echo endpoint flagged 32 and 24 respectively. No listening test was conducted.

## Conditions

The host used an AMD Ryzen AI 9 HX 370 on x86-64 Linux 7.0.0-42. All three processes shared logical CPUs 8–11. Rust 1.98.1 built release binaries at optimization level 2 with LTO and debug information disabled. RustPBX and sipbot both used their published lockfiles and disabled default features. The candidate replaced only the raw decoder inside the published audio-codec 0.4.6 adapter. It retained the encoder, channel switching, scratch buffers, sample scale and stereo-to-mono downmix. Opura's signed PCM was converted to the float scale expected by the existing adapter. sipbot 0.2.82 explicitly bound its media sockets to loopback in both runs. [Stock audio adapter](https://github.com/restsend/audio-codec/blob/8530129da137b6db6b910e24b2d81b5b3ea711a9/src/opus.rs)

The runner sampled the PBX process every 250 ms. Idle memory was sampled after the echo endpoint registered and before the caller started. Live memory is the median from seconds 3–9 after starting the caller, when all eight calls were active. CPU is the change in user plus system CPU ticks divided by elapsed wall time across that interval. The client and echo processes are excluded. The single pair gives no estimate of run-to-run variation. An initial stock attempt at 16 calls returned one SIP 487 and only 15 media bridges, so it was excluded. A separate WebRTC attempt in the loopback namespace returned SIP 200 with no ICE candidate in the PBX answer and delivered no audio. This report covers the SIP/RTP path.

## Reproduce

Run from the Opura repository on Linux with Python 3.12+, Cargo, Rust 1.98.1, a C toolchain, protobuf compiler and bubblewrap. Choose four available CPUs if the recorded affinity is unavailable. The build downloads the release archives and speech fixture and checks their SHA-256 digests. The scripts keep build products, separate SQLite databases, call logs, sample traces and tone recordings in the scratch directory.

```sh
tool="$PWD/tools/benchmarks/rustpbx-call"
scratch="$(mktemp -d)/call"
RUSTUP_TOOLCHAIN=1.98.1 taskset -c 8-11 python3 "$tool/prepare.py" "$scratch"
for calls in 1 8; do
    for backend in stock opura; do
        bwrap --unshare-net --unshare-pid --die-with-parent \
            --ro-bind / / --bind "$scratch" "$scratch" --proc /proc --dev /dev \
            -- taskset -c 8-11 python3 "$tool/call.py" "$scratch" "$backend" \
                "$backend-$calls" --calls "$calls"
    done
done
```

The runner checks SIP success counts, bridge activations and audio received at both endpoints. Single-call runs also check each incoming WAV for the source tone. [Build and run scripts](../../tools/benchmarks/rustpbx-call)
