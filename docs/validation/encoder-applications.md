# Encoder applications and automatic file encoding

Version 0.1.91 extends the common `encoder::OpusEncoder` owner to AUDIO, VOIP,
restricted low delay, restricted SILK and restricted CELT. `new` remains AUDIO;
`with_application` selects an immutable application. Reset preserves it, and a
mismatched per-call application fails without changing any stream state.

The supported API rates remain 8/12/16/24/48 kHz with mono/stereo signed16 PCM.
Single frames are 2.5/5/10/20 ms; restricted SILK requires 10/20 ms. Normal
AUDIO/VOIP use 4 ms input compensation and retain 10 ms filtered PCM. Restricted
low delay uses no compensation but retains history. Restricted SILK/CELT use
neither compensation nor retained outer PCM. Reported lookahead is 312 samples
at 48 kHz for AUDIO/VOIP/restricted SILK and 120 for low delay/restricted CELT.

VOIP connects the previous actual SILK pitch lag, selected signal type, input
quality and activity to the first cutoff smoother, then the outer smoother and
per-API-channel fixed-point biquad. Entering speech initializes the first
smoother before outer filtering. Rate/prediction resets and full reset preserve
or clear the corresponding native histories. Other applications retain DC
rejection. Restricted SILK bandwidth switches do not append CELT tails.

Embedded CELT now signals CBR digital silence, reserving its remaining entropy
budget without extra symbols or VBR shrinking. Prior overlap participates in
the decision. Existing standalone fixed-mode defaults are preserved.

## Automatic WAV/Ogg CLI

`--mode auto` routes actual PCM through this common owner. It accepts
`--application audio|voip|lowdelay|restricted-silk|restricted-celt`,
`--signal auto|voice|music`, `--complexity 0..10`, `--channels 1|2`, bandwidth,
bitrate, CBR/VBR/CELT CVBR, DTX, FEC and packet caps. The Ogg pre-skip follows
the application; zero padding flushes lookahead and the final granule preserves
exact empty/partial-file duration. Existing output paths are refused.

```sh
cargo run --offline --locked --release --bin opura -- encode input.wav output.opus \
  --mode auto --application voip --bitrate 32000 --vbr --fec 10
```

Automatic long packets are deliberately rejected before output creation.
Native whole-packet mode decisions and first/last transition placement still
need implementation; grouping independently selected frames would be wrong.
Existing fixed-mode grouping remains available. Coding failures may leave a
partial new output, as documented for the existing CLI.

## Independent validation

Optional reference generators use unchanged Opus 1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed fast64/Q24 with float
analysis disabled. Reference executables are validation tools only; Cargo has
zero external dependencies.

* 5,120 original VOIP/restricted application snapshots check wire/ramp decisions, active
  resamplers, cumulative speech PCM, both cutoff smoothers, lookahead, retained
  delay and every actual CELT call (duration, band, prediction, capacity and
  complete input PCM hash). These use complexity zero. Restricted CELT has no
  speech state; wrappers do not read its absent allocation.
* 640 direct original cases independently vary previous pitch, quality,
  activity and filter memories. They check first/second smoother, cutoff,
  all resulting biquad memories and complete output PCM. The natural
  application snapshot corpus keeps the first smoother at its minimum, so
  these direct cases supply the separate adaptation evidence.
* 2,560 original CELT silence/resume rows check silence schedules. All 662
  quiet packets (331 CBR, 331 VBR) match complete original bytes and final
  range; all rows additionally decode through the common decoder.
* Four isolated wrong implementations are detected: omitted silence-budget
  accounting, altered biquad coefficient, altered adaptation coefficient and
  normal compensation incorrectly applied to restricted input. Source was
  restored before final checks.
* Actual CLI integration covers 300 empty/partial WAV files across all rates,
  both API layouts, all applications and CBR/VBR. It checks exact decoded
  duration and pre-skip. Five longer cases cover controls, FEC/DTX, mono wire
  and caps; invalid long packets and invalid controls fail before creation.

Reproduce original fixtures outside Cargo dependencies:

```sh
python3 tools/opus_application_fixtures.py target/reference/opus target/app-input.txt \
  --calls-output target/app-calls.txt --hp-output target/voip-hp.txt
python3 tools/opus_silence_fixtures.py target/reference/opus target/celt-silence.txt
cargo test --offline --locked --lib encoder::stream
cargo test --offline --locked --test audio_cli automatic_
```

The separate `application_encoder_vectors` example records actual emitted
packets, decoded PCM hashes and ranges in 100 streams/9,600 received calls. It
covers all five applications and rates, CBR/VBR/CVBR, complexity 0/6/10,
automatic/explicit mode and wire selection, short frames, DTX, FEC and losses.
The encoded schedule contains 3,264 SILK, 696 Hybrid and 5,640 CELT packets,
700 wire changes and 300 DTX packets; received calls include 800 PLC and 800
FEC requests. Decoding these packets through unchanged native decoding checks
PCM and final ranges. This does not establish encoder byte or quality parity.

```sh
cargo run --offline --locked --release --example application_encoder_vectors -- target/apps.bin
python3 tools/silk_mono_validation.py target/reference/opus target/apps.bin
```

The [versioned evidence report](results/encoder-applications-0.1.91.json)
records final corpus identity, repeat/native replay and offline gates. Fixed
speech/Hybrid corpus identity is recorded separately in
[the fixed-path report](results/encoder-applications-0.1.91-fixed.json).

All 24 complete fixed SILK/Hybrid corpora match committed 0.1.90 hashes,
byte counts and record statistics: 1,384,320 records in 5,088 streams,
220,896 FEC requests and 768 PLC calls. This repeats prior native-validated
bytes; native replay of those old corpora was not repeated. All seven required
offline gates pass, including 155 library tests and 23 Python tests.

## Remaining boundary

The full codec goal remains open: native long-packet scheduling, native
insufficient-budget PLC admission, remaining quality/complexity/rate-control
review, and competitive end-to-end evaluation on representative recordings.
Input/call agreement and decoded PCM agreement are compatibility evidence,
not evidence of perceptual superiority, encoder byte parity or SOTA speed.
