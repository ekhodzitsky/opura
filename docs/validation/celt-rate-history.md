# Native CELT rate and intensity continuity

Version0.1.95 fixes a shared-encoder state error exposed by switching between
CBR, VBR and constrained VBR under varying physical budgets. CELT allocation
can clip the requested intensity-stereo boundary to the bands actually coded.
Native allocation writes this realized boundary back to the state used by the
next frame's hysteresis. Opura previously retained the requested boundary.
The shared owner now retains `allocation.intensity()`, including zero after
mono coding. Legacy fixed public paths keep their existing policy.

A deterministic12kHz stereo stream first exposed different final range and
PCM at frame154 (zero-based):20ms, complexity9, VBR9600bps and a24-byte payload
cap. Rate feedback and packet length already matched. Entropy checkpoints
matched before spectral coding; band tracing found native intensity7 versus
Rust intensity8, causing the dual-stereo/intensity switch one band too late.
The fix preserves the actual boundary rather than changing the threshold table.

## Actual native histories

The oracle remains pinned to Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. Its encoder has only the disclosed
private Q24 local-type correction from [the preceding audit](celt-quality.md).
The original checkout and decoder stay unchanged. Instrumentation reads state
without replacing allocation, spectrum, entropy or rate calculations.

The new5,132-call corpus contains22 live streams:

- Five input rates and1/2 API channels, each with two192-frame sequences.
  They switch CBR/VBR/CVBR every six frames, change2.5/5/10/20ms windows,
  exercise complexity0..10 and physical caps2..1275. VBR/CVBR nominal targets
  selected from16..510kbps are limited by the physical cap. CBR uses that cap
  with CELT BITRATE_MAX, as supplied by the shared owner. Twelve-frame silence
  runs recur before nonzero resumption.
- One1100-frame48kHz mono CVBR stream exercises the live feedback count through
  its970-frame saturation and steady-state alpha branch.
- One192-frame48kHz stereo API stream changes mono/stereo wire coding while
  retaining its input/layer/rate histories, checking the zero mono boundary
  and stereo resumption explicitly.

The ordinary Rust regression compares every complete payload byte, decoded
PCM hash, final range, realized intensity, spreading decision, tonal/HF
averages, tapset, temporal spectral average and VBR reservoir/drift/offset/count.
All5,132 packets are byte-identical to the corrected encoder oracle. Exact
packet bytes and original native decoding support the PCM/range comparison;
this is control compatibility, not evidence of perceptual or speed superiority.
The prior10,560-frame quality and768-helper fixtures regenerate unchanged.
The intensity fix also eliminates the three payload differences recorded for
0.1.94: all10,560 previous quality packets now match every byte, and the
regression requires complete byte identity. Their PCM/ranges still match.

Generate all three optional fixtures using the clean reference:

```sh
python3 tools/celt_quality_fixtures.py target/reference/opus \
  target/celt-quality.txt --patch-output target/celt-patch.txt \
  --correct-q24 --rate-output target/celt-rate-history.txt
cmp target/celt-quality.txt tests/celt-quality-corrected-q24-fixtures.txt
cmp target/celt-patch.txt tests/celt-patch-corrected-q24-fixtures.txt
cmp target/celt-rate-history.txt tests/celt-rate-history-corrected-q24-fixtures.txt
cargo test --offline --locked --lib rate_history_tests
```

No external compiler or codec is required by normal Cargo tests. C is only
an optional external validation executable/library; it is not linked, vendored
or declared as a Rust dependency.

The [versioned evidence report](results/celt-rate-history-0.1.95.json) records
fixture identity, detected mutations, required checks and independent shared
owner corpus decoding. Full native-control review and representative matched
rate/latency perceptual, throughput and memory comparisons remain work.

## Recording and whole-owner validation

The unchanged native decoder independently replays the complete short, long
and budget-pressure shared-owner corpora:9,600 calls/100 streams each, including
800 explicit PLC and800 FEC requests. Their byte hashes remain identical to
0.1.94; the new regression reaches a history combination absent from those
existing corpora. Fixture generation repeats identically, and three isolated
mutations are detected and restored: missing realized-boundary retention,
retaining stereo intensity during mono and premature VBR-count saturation.

The [recording report](results/celt-rate-history-0.1.95-recordings.json) compares
0.1.94 with0.1.95 on the two complete checksum-pinned speech/music recordings
used by the preceding audit. Twelve paired automatic AUDIO20ms CBR cases cover
32/64kbps and complexity0/5/10. Each of24 outputs must match native PCM, exact
input duration/layout and80/160-byte packet budgets. All12 paired payload streams and decoded PCM remain identical to0.1.94;
the corrected history combination occurs in the new switching regression.
These are waveform regressions on two recordings, not a representative
perceptual evaluation.

Reported encode wall times include process/file overhead. One warmup precedes
three measured runs with alternating baseline/candidate order. Checks and
fixture jobs finish before this diagnostic run; CPU frequency/affinity are not
controlled. No core throughput, latency/memory or competitor conclusion follows.
The earlier fixed-corpus regeneration is not repeated for this native-only
history correction; fixed library/CLI regressions run in the required gates.
