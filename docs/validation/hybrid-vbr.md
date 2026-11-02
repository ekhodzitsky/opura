# Unconstrained Hybrid variable bitrate

Version 0.1.78 adds `HybridEncoder::encode_vbr(duration, pcm, bitrate,
maximum_packet_bytes, options)` and CLI `--mode hybrid --vbr`. The nominal
bitrate includes the TOC and is separate from the hard packet capacity, which
defaults to 1276 bytes in the CLI. Existing CBR APIs and defaults retain their
bytes. DTX and in-band FEC remain opt-in and can be combined with VBR.

## Target and state contracts

The nominal rate must supply at least 24 bits per frame and be at most
1024000 bits/s. Physical capacity can reduce that target. The selected CELT
high-band target must exceed 500 bits/s, matching the native bitrate-control
domain. These bounds are admission prerequisites, not a guarantee that every
fixed-band frame fits. Unattainable targets and budgets return explicit errors.

Hybrid uses unconstrained high-band VBR, as in the original encoder: no CELT
constrained-VBR reservoir or drift feedback is applied. Nominal high-band rate
controls analysis and the target; actual SILK quantization offset and temporal
analysis bias that target, with the original transient floor. The shared writer
shrinks after dynamic requests and trim, before anti-collapse, fine energy and
PVQ allocation. Length includes actual entropy usage, boost reservations, the
two-byte margin and the shared 37-bit redundancy-syntax floor. Raw suffix bits
move with the new capacity. Native frame limits cap payloads to 637 bytes for
10 ms and 1275 bytes for 20 ms, in addition to the caller's limit.

SILK rate allocation and fixed-band FEC eligibility use VBR policy. Existing
early SILK versus outer digital DTX ordering remains intact. CBR/VBR calls can
alternate on one encoder. Both layers, pending protection and all preprocessing
histories roll back together on error, including failures after shrinking.
Entropy termination runs once; pending-prefix admission retries remain bounded.

## Evidence

The unchanged reference is scalar fixed-point Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, with compilation settings described
in [normal Hybrid validation](hybrid-pcm-packets.md). Reference sources and
executables are optional validation inputs, never Cargo dependencies.

- 20160 unchanged-original target/minimum-size arithmetic snapshots cover
  frame sizes, channels, offsets, temporal thresholds, capacities and shared
  entropy/boost contexts. Extreme arithmetic inputs include a one-bit/s kernel
  boundary; this does not bypass the public target-domain checks. Regeneration
  is byte-identical. An additional 25600 original VBR FEC policy decisions
  match, alongside the unchanged 25600 CBR cases.
- 1152 actual streams produce 184320 independently decoded records with exact
  PCM and final range, including 30528 FEC requests, 10650 requests whose packet
  advertises any LBRR, 54042 protected primary records, 25534 primary DTX records
  and 5304 DTX-input FEC fallbacks. Coverage includes eligible 24/48 kHz rates,
  mono/stereo, SWB/FB, changing 10/20 ms geometry, complexity 0/6/10, nominal
  rates, tight capacities, high-rate boundaries, CBR/VBR toggles, protection and
  DTX toggles, silence/quiet/speech and stereo changes. FEC presence does not
  guarantee successful recovery of every requested channel/frame. See
  [raw results](results/hybrid-vbr-0.1.78-vectors.json).
- 48 actual speech/music CLI files cover VBR alone, VBR/FEC and VBR/FEC/DTX,
  with SWB/FB, 10/20 ms and fractional 32001/48001/64001-bit/s targets.
  Each 16-file profile has 60976 packets. FEC alone carries LBRR in 56274
  packets; FEC/DTX carries it in 56275 packets and suppresses 1286 packets.
  Packet lengths vary, caps hold, and independent native PCM and exact duration
  after 312 samples of pre-skip match. See [normal files](results/hybrid-vbr-0.1.78-normal-files.json),
  [FEC files](results/hybrid-vbr-0.1.78-fec-files.json) and
  [FEC/DTX files](results/hybrid-vbr-0.1.78-fec-dtx-files.json).

Ordinary tests exercise invalid targets, tiny/maximum caps, actual variable
lengths, duration, argument rejection, changing geometry and late-failure whole
state rollback. All seven required offline gates pass, including 23 Python
tests. The old normal (5760 records), DTX (21600 records) and CBR/FEC (184320
records) raw Hybrid corpora retain their exact earlier hashes. Six current
SILK/CELT full-recording regressions retain audio packet and WAV bytes against
the verified 0.1.77 outputs, themselves identical to 0.1.76; see
[regressions](results/hybrid-vbr-0.1.78-regressions.json).

## Reproduction and limits

```sh
python3 tools/hybrid_encoder_vbr_fixtures.py /path/to/opus-v1.6.1 /tmp/hybrid-vbr.txt
diff -u tests/hybrid-encoder-vbr-fixtures.txt /tmp/hybrid-vbr.txt
python3 tools/hybrid_encoder_fec_policy.py /path/to/opus-v1.6.1 /tmp/hybrid-vbr-fec.txt --vbr
diff -u tests/hybrid-encoder-vbr-fec-policy-fixtures.txt /tmp/hybrid-vbr-fec.txt
cargo run --offline --locked --release --example hybrid_fec_vectors -- /tmp/new-hybrid-vbr.bin vbr
python3 tools/hybrid_encoder_validation.py /path/to/opus-v1.6.1 /tmp/new-hybrid-vbr.bin /tmp/hybrid-vbr.json --profile vbr
cargo build --offline --locked --release
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-vbr-files --vbr
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-vbr-fec-files --vbr --fec 20
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-vbr-fec-dtx-files --vbr --fec 20 --dtx
```

Raw recordings, vectors, binaries and logs stay local; tracked reports retain
sanitized hashes/counts. These results compare independent decoders of Opura
output, not native encoder quality or speed, perceptual quality or long-term
bitrate accuracy. Fixed-band Hybrid is now usable with CBR/VBR/DTX/FEC. General
mode/channel adaptation, CELT VBR/CVBR, longer packet orchestration and competitive
end-to-end evaluation remain unfinished in the full-codec goal. No SOTA claim
follows from interoperability or the two-recording diagnostics.
