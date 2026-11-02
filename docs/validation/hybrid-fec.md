# Shared-stream Hybrid in-band FEC

Version0.1.77 adds opt-in LBRR protection to the existing Hybrid encoder and CLI.
Call `set_inband_fec(true)` and supply `Options::packet_loss_percent`, or use
`--mode hybrid --fec 20`. Protection covers the previous frame's SILK band;
CELT high-band recovery uses concealment. `has_fec()` reports actual carried
LBRR, rather than merely the requested setting. DTX and FEC can be combined.
Reset retains both settings and clears all redundant/main histories.

## Fixed-band policy and shared budgets

The native Hybrid equivalent-rate calculation includes TOC-inclusive rate,
10ms overhead, CBR, complexity and loss penalties. FEC uses original SWB/FB
thresholds and hysteresis. The general original frontend can lower bandwidth
or switch to SILK to enable protection. This fixed-band API accepts FEC only
when that decision keeps the requested SWB/FB. It never silently changes mode
or band. Low bitrate/activity can therefore leave protection unavailable.

The selected FEC policy also drives Hybrid's native SILK/high-band rate split.
Pending LBRR is written before primary data into the same unfinished range
writer. Existing SILK redundant NSQ, gain history, masks, stereo predictors and
moving-average bit accounting are reused. Incompatible10/20ms pending geometry
is cleared; disabling new protection can still carry a queued backup once.
Digital-zero outer inactivity prevents capturing a new LBRR even when a voiced
resampler/VAD tail remains. Side omission/restart and both DTX paths retain their
existing state ordering.

A pending-prefix budget failure permits exactly one retry from identical whole
stream state without the old prefix. It can preserve primary audio by dropping
that protection. Frame preparation is separate from entropy termination, so
finalization executes once and is never retried. Errors preserve all input,
analysis, predictor, redundant, stereo, DTX and CELT state. Tiny budgets can still
fail explicitly. This is a bounded admission policy, not a promise of universal
backup coverage or improved quality.

## Verification

The unchanged original reference is Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, using the scalar fixed-point flags
listed in [normal Hybrid validation](hybrid-pcm-packets.md). Native codec code
remains optional validation only, outside Cargo and ordinary tests.

- 25600 policy snapshots match original `compute_equiv_rate`/`decide_fec`,
  accepting only decisions retaining the requested band. They cover bands,
  channels, durations, previous selection, enabled state, complexity, loss and
  tiny/maximum budgets. Deterministic regeneration reproduces the fixture.
- 1152 streams produce184320 native cross-decoder records, including30528 FEC
  requests. They cover all eligible rate/band pairs, mono/stereo,10/20ms,
  complexity0/6/10,32/48kbps per channel, loss estimates0/5/20/100, protection
  toggles, DTX off/on, silence/quiet/speech, stereo phase changes, duration
  changes and reduced/increased budgets. Loss replay omits every third primary
  frame and requests it from the next received packet, then decodes that packet
  normally. Unavailable protection exercises PLC fallback. Actual PCM and public
  final range match the unchanged native decoder. Presence counts mean any
  advertised LBRR, not necessarily full stereo or multi-duration recovery. See
  the [raw result](results/hybrid-fec-0.1.77-vectors.json).
- 32 actual CLI speech/music files cover FEC alone and FEC+DTX, SWB/FB,10/20ms
  and fractional32001/48001/64001bps budgets. Each16-case matrix has60976
  packets and53134 packets carrying LBRR; the DTX matrix also has1279 suppressed
  packets. CLI WAV PCM equals independent native decoding with exact312 pre-skip
  and original duration. See [FEC files](results/hybrid-fec-0.1.77-files.json) and
  [FEC/DTX files](results/hybrid-fec-dtx-0.1.77-files.json).

Ordinary tests check FEC advertisement, primary/recovery decoding, DTX, geometry
changes, reset retention, inactive tails, successful prefix dropping, explicit
admission failures and complete rollback. The retry result/state equals an
explicitly prefix-free attempt; finalization runs once even when injected to
fail. Eight actual CLI FEC/DTX silence/resume cases preserve exact duration.
The old5760-record normal Hybrid and21600-record DTX corpora remain byte-identical.
Six full-recording SILK/CELT checks preserve audio packets and WAV bytes against
the archived verified0.1.76 binary; see [regressions](results/hybrid-fec-0.1.77-regressions.json).
Existing fixture expectations remain unchanged. All seven repository gates pass,
including23 Python tests.

## Reproduction and limits

```sh
python3 tools/hybrid_encoder_fec_policy.py /path/to/opus-v1.6.1 /tmp/hybrid-policy.txt
diff -u tests/hybrid-encoder-fec-policy-fixtures.txt /tmp/hybrid-policy.txt
cargo run --offline --locked --release --example hybrid_fec_vectors -- /tmp/new-hybrid-fec.bin
python3 tools/hybrid_encoder_validation.py /path/to/opus-v1.6.1 /tmp/new-hybrid-fec.bin /tmp/hybrid-fec.json --profile fec
cargo build --offline --locked --release
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-fec-files --fec 20
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-fec-dtx-files --fec 20 --dtx
```

Raw recordings, listening outputs, vectors, binaries and logs remain local.
Tracked results contain sanitized hashes/counts. Recording checks compare two
decoders of actual Opura output, not native encoder quality or speed. Synthetic
loss masks do not establish representative network or perceptual performance.
Hybrid VBR, general mode/channel orchestration, longer packet assembly and
competitive end-to-end evaluation remain unfinished in the full-codec goal.
