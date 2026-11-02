# CELT VBR and constrained VBR

Version 0.1.79 adds `CeltEncoder::encode_vbr(size, pcm, VbrOptions)` and CLI
`--vbr` / `--cvbr`. The library's hard `payload_bytes` capacity excludes the
one-byte TOC, matching the existing CELT API. CLI `--max-packet-bytes` includes
TOC and defaults to 1276. Existing CBR APIs, policies and defaults retain their
bytes. The library's analysis/prefilter switches still apply; the CLI enables
all six native analyses.

## Rate, silence and state contracts

The nominal bitrate includes TOC. It must supply at least 24 bits per frame and
be at most 1500000 bits/s. Native per-channel limits (750000 bits/s/channel)
and physical capacity can reduce it. Payload capacity is 2..1275 bytes.
These are admission bounds: unattainable budgets can still fail explicitly.

The packet-owning API deducts eight TOC bits per frame from the VBR target,
using the original native CELT signalling accounting. Equivalent-rate analysis
retains the full stream rate and original physical capacity. This distinction
matters especially for short frames: charging the payload reservoir the full
stream rate would add 3200 bits/s at 2.5 ms. Initial recording diagnostics caught
that error before publication; all retained results use the corrected budget.

The native fixed-point target incorporates coded-band stereo savings, intended
dynamic boosts, TF calibration, signal depth and temporal spectral history.
Constrained VBR applies the original capacity cap before syntax is written,
then updates reservoir, drift, offset and saturating count after trim. Both
variable policies choose final capacity after actual boost/trim syntax and
before anti-collapse/fine/PVQ allocation. Raw suffix bits relocate with the
capacity. Native per-frame payload ceilings are 159/318/637/1275 bytes for
2.5/5/10/20 ms, alongside the caller's hard limit.

Digital-zero silence includes the preceding raw input overlap. It uses a
two-byte CELT payload plus TOC, advances filtering/analysis and restores active
reconstructed energies to the native silence floor. CBR retains its previous
silence=false behavior. Spectral history continues through CBR; VBR/CVBR and
CBR calls, rates and frame durations can alternate. Reset clears feedback and
audio histories while retaining configured analysis switches. Every failed
call preserves the entire encoder, including post-shrink termination failures.

## Verification

The unchanged external reference is scalar fixed-point Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, with the same settings as
[the Hybrid validation reference](hybrid-pcm-packets.md). The optional target
oracle extracts unchanged `compute_vbr`, pre-cap and post-control feedback
blocks, supplying native signalling overhead in its packet-owning context.
It observes arithmetic capacities, not native entropy bytes.

- 8192 original target/capacity/feedback decisions match exactly across eight
  1024-frame traces, including count saturation at 970, changing rates,
  capacities, durations, channels, stereo savings, measured depth, TF/temporal
  inputs, silence and constrained toggles. An additional 1024 unchanged-native
  temporal-history snapshots cover full and high-band mono/stereo intervals.
  Both fixtures regenerate byte-identically.
- 80 actual streams produce 10240 independent native-decoder records with
  exact PCM and final range, including 440 lost packets and 2299 received
  silence packets. They cover all five standard input rates, mono/stereo,
  changing 2.5/5/10/20 ms geometry, CBR/VBR/CVBR toggles, nominal rates,
  reduced capacities, high-rate boundaries, silence/quiet/speech and stereo
  changes. Loss omits frames where `frame % 11 == 7`. See
  [raw results](results/celt-vbr-0.1.79-vectors.json).
- 32 actual speech/music CLI files cover both variable policies, all four
  frame durations and fractional 32001/64001/128001-bit/s targets. Each
  16-file profile contains 152408 packets. Independent native PCM, 120-sample
  pre-skip, duration, variable lengths and packet caps match. See
  [VBR files](results/celt-vbr-0.1.79-files.json) and
  [CVBR files](results/celt-cvbr-0.1.79-files.json).

Across these recordings, CVBR packet bitrate relative to its nominal target
ranges from 0.999992 to 1.000863 (less than 0.09% error). Unconstrained VBR
ranges from 0.974403 to 1.204129. The denominator is the coded frame timeline;
packet bytes include TOC and exclude Ogg. These measurements describe this
corpus, not a universal average-rate guarantee, especially under silence,
tiny budgets, short streams or changing targets.

Ordinary tests cover all geometries, actual silence/resume, previous raw
overlap, hard caps, invalid rates, reset retention, changing policies and full
state rollback after shrinking/finalization. Eighty actual CLI cases preserve
duration across rates/channels/durations/policies; conflicting flags fail before
creating output. Existing invalid analysis-domain tests remain unchanged.
Six full SILK/CELT recording regressions retain packet and WAV bytes against
verified 0.1.78. All four previous Hybrid raw corpora (normal, DTX, CBR/FEC and
VBR/FEC) retain their earlier hashes. See
[regressions](results/celt-vbr-0.1.79-regressions.json).

All seven required repository gates pass, including complete debug/release
suites, denied Clippy/rustdoc warnings, the dependency audit and 23 Python tests.
No Cargo dependency or unsafe exception is introduced.

## Reproduction and limits

```sh
python3 tools/celt_encoder_vbr_fixtures.py /path/to/opus-v1.6.1 /tmp/celt-vbr.txt --temporal-output /tmp/celt-temporal.txt
diff -u tests/celt-encoder-vbr-fixtures.txt /tmp/celt-vbr.txt
diff -u tests/celt-encoder-temporal-vbr-fixtures.txt /tmp/celt-temporal.txt
cargo run --offline --locked --release --example celt_vbr_vectors -- /tmp/new-celt-vbr.bin
python3 tools/celt_vbr_validation.py /path/to/opus-v1.6.1 /tmp/new-celt-vbr.bin /tmp/celt-vbr.json
cargo build --offline --locked --release
python3 tools/celt_vbr_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-celt-vbr-files
python3 tools/celt_vbr_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-celt-cvbr-files --constrained
```

Raw recordings, vectors, binaries and logs remain local. Tracked results retain
sanitized hashes/counts. Decoder equality does not prove native encoder byte
identity, perceptual quality or speed. The fixed-point target omits native
floating-analysis, surround, LFE and QEXT features outside this API's scope.
General mode/channel adaptation, longer packet orchestration and competitive
end-to-end evaluation remain unfinished in the full-codec goal. These results
do not establish SOTA or superiority over libopus.
