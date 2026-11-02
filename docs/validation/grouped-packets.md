# Multi-frame Opus packet assembly and file encoding

Version 0.1.81 adds allocation-free library assembly through
`repacketizer::Repacketizer` and actual CLI grouping with `--packet-ms`.
The maximum duration is 120 ms; the requested duration must be a positive
multiple of the existing `--frame-ms`. Defaults still emit one encoder frame
per packet. CELT, SILK and Hybrid keep their current per-frame policies and
continuous DTX/FEC histories.

```sh
target/release/opura encode input.wav grouped.opus --mode celt --frame-ms 20 --packet-ms 120 --bitrate 128001
target/release/opura encode input.wav grouped-vbr.opus --mode hybrid --vbr --frame-ms 20 --packet-ms 120 --bitrate 64001 --max-packet-bytes 1200 --fec 20 --dtx
target/release/opura encode input.wav grouped-silk.opus --mode silk --bandwidth auto --frame-ms 20 --packet-ms 100 --fec 20 --dtx
```

## Framing, rates and failure contracts

The library borrows up to 48 validated payload frames. Appended packets must
share TOC configuration and encoded channel count; the total duration cannot
exceed 5760 samples on the 48 kHz clock. Empty compressed frames are valid.
Canonical code0/1/2/3 output uses one/two-byte sizes where required. Range
selection and exact transport padding are supported. Append failure preserves
all frames; write failure preserves the destination and assembly. Successful
writes leave the unused destination suffix untouched. Input transport padding
is discarded. Self-delimited framing, multistream and padding extensions are
outside this API. Parsing/assembly validates framing, not compressed audio.

CLI grouping reserves conservative framing space in each component encoder's
physical quota. CBR credit uses the actual duration of each emitted packet,
including TOC, length fields and transport padding. All-DTX groups avoid CBR
padding and contain at most two bytes. Variable policies retain their native
frame targets with a hard aggregate byte cap; capacity limits can reduce the
target. Their average bitrate is not universally guaranteed. The default
aggregate cap is 1276 bytes per planned component, up to 61248; an explicitly
provided cap cannot exceed the corresponding frame-count capacity. Encoder
packets fit one bounded Ogg page; decoding retains its existing 65536-byte
command limit.

The last partial group closes at the final encoded frame. A changed SILK TOC
also closes the previous compatible group, preserving automatic bandwidth
histories rather than resetting the encoder. Thus `--packet-ms` is a maximum
compatible group duration. Fractional CBR credit continues across early closes.
Pre-skip remains 120 samples for CELT and 312 for SILK/Hybrid; final granules
preserve the exact resampled source duration, including empty/partial input.
Existing outputs remain protected; a file error can leave a partial new output.
Grouping uses bounded CLI-owned buffers; existing frame encoders and the
borrowing library assembler retain their no-allocation contracts.

The first frame's LBRR can protect only the final frame of a missing long
interval. Earlier samples use PLC. A FEC request is not a guarantee that any
protection is available. The received packet must still be decoded normally
for its primary audio, as before.

## Verification

The unchanged external reference is scalar fixed-point Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. Native sources/executables are
optional validation only; ordinary builds/tests have zero Cargo dependencies.

- 32816 complete-byte fingerprints match the original public repacketizer and
  padding API across all 64 configuration/channel TOCs, compatible frame
  counts up to 48/120ms, range selection, zero-length frames, 251/252-byte size
  transitions, maximum 1275-byte frames, and padding deltas
  0/1/2/254/255/256/511. Deterministic regeneration is byte-identical.
- Ordinary tests verify malformed/mixed/overflow append errors, invalid
  ranges, insufficient capacities, unchanged output/state on failure, reset,
  grouped/padded flattening and untouched output suffixes. A configuration
  change closes compatible packets while preserving exact total CBR credit.
  Large Ogg packet tests cover lacing, CRC and granules through 61248 bytes.
- 216 actual CLI geometry/policy cases cover all five input rates,
  mono/stereo, CELT CBR/VBR/CVBR, fixed/automatic SILK and Hybrid, FEC/DTX and
  packet durations including 7.5/20/60/80/100/120ms. Additional empty/partial
  and silence/resume cases preserve duration and compact all-DTX groups.
  Invalid durations/caps fail before creating output.
- 34 actual speech/music CLI files contain 33636 packets, including 84
  all-DTX groups and 5913 packets advertising LBRR. Independent native PCM,
  pre-skip/duration, compatible framing, exact CBR budgets and variable caps
  match. The largest observed packet is 1960 bytes. In three replay schedules,
  102 streams / 9672 decoder records match native PCM and final range,
  including 238 explicit losses and 298 FEC requests; 78 requests have any
  advertised LBRR. Presence does not imply full-interval protection. See
  [recording/replay results](results/grouped-audio-0.1.81.json).
- 18 full-recording default regressions compare current output against a
  separately rebuilt annotated 0.1.80 source. CELT CBR/VBR/CVBR,
  fixed/automatic SILK, and Hybrid CBR/VBR/FEC/DTX retain exact audio packet
  and WAV bytes. Version-tag metadata is excluded from packet comparisons.
  See [default regressions](results/grouped-default-0.1.81-regressions.json).

All seven final-source offline gates pass: formatting, complete debug/release
tests, all-target Clippy and rustdoc with denied warnings, dependency audit,
and 23 Python tests. Existing native fixture expectations remain unchanged.

## Reproduction and limits

```sh
python3 tools/repacketizer_fixtures.py /path/to/opus-v1.6.1 /tmp/repacketizer.txt
diff -u tests/repacketizer-fixtures.txt /tmp/repacketizer.txt
cargo build --offline --locked --release --bin opura --example packet_replay
python3 tools/grouped_audio_validation.py /path/to/opus-v1.6.1 target/release/opura target/release/examples/packet_replay /path/to/hash-pinned-recordings /tmp/new-grouped-files
```

Raw recordings, vectors, binaries and logs stay local. Tracked reports retain
sanitized hashes/counts. Library framing limits cover legal RFC6716 packets;
the CLI uses documented bounded per-frame quotas and a maximum compatible group
duration. This is not native encoder byte parity or an optimal joint allocation
claim. Synthetic loss schedules and two recordings do not establish perceptual
quality, representative network behavior, speed leadership or SOTA. General
cross-mode/channel adaptation and competitive end-to-end evaluation remain
unfinished in the full-codec goal.
